const MARGIN_KINDS = ['add_margin_note', 'remove_margin_note'];
const READ_GAP_SECONDS = 90;
const READ_FACE_AFTER_MS = 200;
const MARGIN_SETTLE_MS = 1400;
const REANCHOR_MS = 2000;
const FIND_SETTLE_MS = 400;
const MARGIN_LAYOUT_KEY = 'leo-margin-layout';
const GLOW_MS = 2200;
const NO_COMMENT_MS = 4000;
const PENCIL_LEAST_MS = 650;
const PENCIL_SIDE_MS = 900;
const PENCIL_PX_PER_MS = 1.6;
const READING_FADE_MS = 120;
const QUIET_MS = 2600;
const QUIET_FADE_MS = 500;
const WIPE_MS = 500;
const MENU_MS = MOTION_MS;
const MENU_SLIDE = [{ clipPath: 'inset(0 0 100% 0)', opacity: 0, transform: `scale(${POP_FROM})` }, { clipPath: 'inset(0 0 0 0)', opacity: 1, transform: 'none' }];
const MARGIN_MESSAGE_READS = 1000;
const MARGIN_PASSAGE_READS = 1000;
const marginKit = window.leoMargin;
const READ_PAUSE = marginKit.READ_PAUSE;
const PERSONA_ICON = {
  book: ICON.book,
  target: svg('<circle cx="12" cy="12" r="7.5"/><circle cx="12" cy="12" r="3"/><path d="M12 2.5v3M12 18.5v3M2.5 12h3M18.5 12h3"/>'),
  thumb: svg('<path d="M7 10.5V20H4.5v-9.5z"/><path d="M7 10.5l3.6-6.3a1.9 1.9 0 0 1 3.4 1.6L13 9.5h5.3a2 2 0 0 1 2 2.3l-1.1 6.5a2 2 0 0 1-2 1.7H7"/>'),
  flask: svg('<path d="M9.5 3.5h5M10.5 3.5v5.2L5 18.2A1.6 1.6 0 0 0 6.4 20.5h11.2a1.6 1.6 0 0 0 1.4-2.3l-5.5-9.5V3.5"/><path d="M7.6 14h8.8"/>'),
  globe: svg('<circle cx="12" cy="12" r="8.5"/><path d="M3.5 12h17M12 3.5c2.5 2.6 3.5 5.5 3.5 8.5s-1 5.9-3.5 8.5c-2.5-2.6-3.5-5.5-3.5-8.5s1-5.9 3.5-8.5"/>'),
  bulb: svg('<path d="M9 17.5h6M10 20.5h4"/><path d="M8.2 14.2A6 6 0 1 1 15.8 14.2c-.8.7-1.3 1.6-1.3 2.6v.7h-5v-.7c0-1-.5-1.9-1.3-2.6"/>'),
  cap: svg('<path d="M2.5 9.5L12 5l9.5 4.5L12 14z"/><path d="M6.5 11.5v4.3c1.5 1.4 3.4 2.2 5.5 2.2s4-.8 5.5-2.2v-4.3M21.5 9.5v5"/>'),
  leaf: svg('<path d="M5 19c0-8 5-13.5 14.5-14.5C19 14 13.5 19 5.5 19"/><path d="M5 19l7.5-7.5"/>'),
  compass: svg('<circle cx="12" cy="12" r="8.5"/><path d="M15.5 8.5l-2 5-5 2 2-5z"/>'),
  code: svg('<path d="M8.5 7.5L4 12l4.5 4.5M15.5 7.5L20 12l-4.5 4.5"/>'),
  heart: svg('<path d="M12 19.5s-7.5-4.4-7.5-9.7A4.3 4.3 0 0 1 12 7.2a4.3 4.3 0 0 1 7.5 2.6c0 5.3-7.5 9.7-7.5 9.7z"/>'),
  star: svg('<path d="M12 3.8l2.5 5.1 5.6.8-4.1 4 1 5.6-5-2.7-5 2.7 1-5.6-4.1-4 5.6-.8z"/>'),
  pen: svg('<path d="M4 20l1.2-4.6L16.6 4a2 2 0 012.8 2.8L8 18.4z"/><path d="M14.5 6.1l3.4 3.4"/>'),
  plus: svg('<path d="M12 5.5v13M5.5 12h13"/>'),
};
let personaStartNew = null;
let personaReturn = null;
let personaEditing = null;
let personaDetails = false;
const PERSONA_ICON_NAMES = { flask: 'Flask', globe: 'Globe', bulb: 'Light bulb', cap: 'Graduation cap', leaf: 'Leaf', compass: 'Compass', code: 'Code', heart: 'Heart', star: 'Star', pen: 'Pen', target: 'Target', thumb: 'Thumbs up' };
const LEAST_ASKED = 3;
const SHOWN_MESSAGES = 6;
const LONG_THREAD = 6;
const MARGIN_AREA_MAX = 140;
const CARD_TOP = 14;
const CARD_GAP = 10;
const CARD_TAIL = 4;
let marginPage = { root: app, faded: new Map(), drafted: new Map(), resolved: [], revisits: new Map(), gone: [], adding: new Set(), reopening: new Set(), again: false, s: null, mode: 'ask', threads: [], layer: null, svg: null, els: new Map(), paths: new Map(), marks: new Map(), rects: new Map(), open: null, hover: null, focus: null, draft: null, thinking: new Map(), wanted: null, arriving: new Set(), timer: 0, reading: false, through: null, faceTimer: 0, status: '', brief: false, statusTimer: 0, statusToken: null, readingAt: null, readingSpan: null, readingEl: null, sides: {}, persona: 'reader', saved: null, instructions: '', personaToken: null, wantedPersona: null, personaSlide: null, slide: null, pointer: 0, changing: false, baseline: [], nextRead: 0, observer: null, watch: null, frame: 0, roomy: new Map(), tethers: new Map(), markers: new Map(), newest: null, folds: new Map(), unfolds: new Set(), unfold: null, moved: [], anchoring: new Set(), anchorTimer: 0, finds: new Map(), drawn: null, drawnAt: 0, settle: 0 };
panes[0].margin = marginPage;
let marginPrefs = null;

function marginRemembered() {
  try {
    return localStorage.getItem(MARGIN_LAYOUT_KEY);
  } catch (e) {
    return null;
  }
}

function marginRemember(prefs) {
  marginPrefs = prefs;
  try {
    localStorage.setItem(MARGIN_LAYOUT_KEY, prefs.layout);
  } catch (e) {}
}

function marginSetting() {
  return marginPrefs?.layout || marginRemembered() || 'both';
}

function marginCovered() {
  const panel = chat.isOpen() && $('.chat');
  return panel ? Math.max(0, panel.offsetWidth - parseFloat(getComputedStyle(document.body).paddingRight)) : 0;
}

function marginMostColumn(width) {
  return marginKit.mostColumn({ width: width - marginCovered(), setting: marginSetting(), noteWidth: window.leoTheme.noteWidth() });
}

function marginLayout(columns) {
  const max = getComputedStyle($('article.note', columns)).maxWidth;
  const column = max.endsWith('px') ? parseFloat(max) : Infinity;
  const width = columns.clientWidth - marginCovered();
  const layout = marginKit.arrangement({ width, setting: marginSetting(), noteWidth: window.leoTheme.noteWidth() });
  const size = `${marginKit.marginWidth({ width, column, layout })}px`;
  if (columns.style.getPropertyValue('--margin-w') !== size) columns.style.setProperty('--margin-w', size);
  return layout;
}

async function marginLoadPrefs() {
  if (marginPrefs) return marginPrefs;
  try {
    marginRemember(await api('/api/margin'));
  } catch (e) {
    return { layout: marginRemembered() || 'both', pace: 'regular' };
  }
  return marginPrefs;
}

function marginLeave() {
  clearTimeout(marginPage.timer);
  clearTimeout(marginPage.anchorTimer);
  clearTimeout(marginPage.settle);
  clearTimeout(marginPage.faceTimer);
  clearTimeout(marginPage.statusTimer);
  cancelAnimationFrame(marginPage.frame);
  cancelAnimationFrame(marginPage.pointer);
  if (marginPage.through) marginPage.through.stop = true;
  marginPage.slide?.cancel();
  marginPage.personaSlide?.cancel();
  marginPage.observer?.disconnect();
  marginPage.observer = null;
  marginPage.watch?.disconnect();
  marginPage.watch = null;
  for (const slot of marginPage.layer?.parentElement?.querySelectorAll?.('.margin-slot') || []) slot.remove();
  marginPage.layer?.remove();
  Object.assign(marginPage, { faded: new Map(), drafted: new Map(), loaded: false, s: null, mode: 'ask', threads: [], resolved: [], gone: [], layer: null, svg: null, open: null, hover: null, focus: null, draft: null, reading: false, through: null, status: '', brief: false, statusToken: null, readingAt: null, readingSpan: null, readingEl: null, sides: {}, persona: 'reader', saved: null, instructions: '', personaToken: null, wantedPersona: null, personaSlide: null, slide: null, pointer: 0, changing: false, baseline: [], roomy: new Map(), tethers: new Map(), folds: new Map(), unfolds: new Set(), unfold: null, moved: [], anchoring: new Set(), finds: new Map(), drawn: null, drawnAt: 0 });
  marginPage.els.clear();
  marginPage.paths.clear();
  marginPage.revisits.clear();
  marginPage.marks.clear();
  marginPage.rects.clear();
  marginPage.arriving.clear();
  marginPage.thinking.clear();
  marginPage.wanted = null;
}

async function marginMount(s, current) {
  marginLeave();
  const columns = $('.note-columns');
  if (!columns) return;
  const layer = document.createElement('div');
  layer.className = 'margin-layer';
  layer.setAttribute('aria-label', 'Felix’s margin notes');
  layer.innerHTML = '<svg class="margin-ink" aria-hidden="true"></svg><div class="margin-said-aloud" aria-live="polite"></div>';
  columns.appendChild(layer);
  columns.dataset.margin = marginLayout(columns);
  Object.assign(marginPage, { s, layer, svg: layer.firstChild, baseline: marginKit.blocksOf(s.doc.source()), nextRead: 0 });
  layer.addEventListener('click', marginClick);
  layer.addEventListener('keydown', marginKey);
  layer.addEventListener('focusin', marginFocus);
  layer.addEventListener('focusout', marginFocus);
  layer.addEventListener('submit', (e) => marginSubmit(e).catch(fail));
  layer.addEventListener('input', (e) => e.target.matches('.margin-reply textarea') && marginSized(e.target));
  if (window.ResizeObserver) {
    marginPage.observer = new ResizeObserver(() => marginPage.folds.size || marginPage.unfolds.size || marginRedraw());
    marginPage.observer.observe(columns);
    marginPage.observer.observe($('#doc'));
  }
  if (window.MutationObserver) {
    marginPage.watch = new MutationObserver(() => marginRedraw());
    marginPage.watch.observe($('#doc'), { childList: true });
  }
  $('#doc').addEventListener('focusin', marginRedraw);
  if (s.note.id) return marginLoad(s, current);
  const prefs = await marginLoadPrefs();
  if (!current() || marginPage.s !== s) return;
  marginPage.mode = prefs.default_mode || 'ask';
  marginPage.persona = prefs.default_persona || 'reader';
  marginControl();
  marginRedraw();
}

async function marginLoad(s, current = () => true, id = s.note.id) {
  let view = null;
  try {
    [, view] = await Promise.all([marginLoadPrefs(), api(`/api/notes/${enc(id)}/margin`)]);
  } catch (e) {
    if (e instanceof Locked) throw e;
    return;
  }
  if (!current() || marginPage.s !== s || !view) return;
  const was = marginPage.loaded;
  const shown = (m) => JSON.stringify([m.mode, m.persona || 'reader', m.instructions || '', m.threads || [], m.resolved || []]);
  if (was && shown(view) === shown(marginPage)) return;
  marginPage.mode = view.mode;
  marginPage.persona = view.persona || 'reader';
  marginPage.saved = marginPage.persona;
  marginPage.instructions = view.instructions || '';
  marginPage.threads = view.threads || [];
  marginPage.resolved = view.resolved || [];
  const first = !marginPage.loaded;
  marginPage.loaded = true;
  cancelAnimationFrame(marginPage.frame);
  marginDraw();
  marginControl();
  marginInstructions();
  if (first) marginReanchor(s).catch(() => {});
}

function marginKept(t) {
  return t.source !== 'read' || t.pinned || (t.messages || []).some((m) => m.from !== 'felix');
}

async function marginReanchor(s) {
  if (s !== marginPage.s || !s.note.id) return;
  await flush(s);
  if (s !== marginPage.s || saving.unsaved()) return;
  const source = s.doc.source();
  const version = s.note.version;
  for (const { t, at } of [...marginPage.moved]) {
    if (marginPage.anchoring.has(t.id)) continue;
    marginPage.anchoring.add(t.id);
    try {
      const moved = await api(`/api/notes/${enc(s.note.id)}/margin/threads/${enc(t.id)}`, { method: 'PATCH', body: { version, quote: source.slice(at.from, at.to), ...marginKit.contextAt(source, at.from, at.to) } });
      if (s !== marginPage.s || !moved?.id) continue;
      marginPage.threads = marginPage.threads.map((one) => (one.id === moved.id ? moved : one));
      marginRedraw();
    } catch (e) {
      if (e instanceof Locked) throw e;
    } finally {
      marginPage.anchoring.delete(t.id);
    }
  }
}

async function marginCreated(s, note) {
  if (marginPage.s !== s) return;
  const wanted = marginPage.wanted;
  const persona = marginPage.wantedPersona;
  marginPage.wanted = null;
  marginPage.wantedPersona = null;
  await marginLoad(s, undefined, note.id);
  if (persona && marginPage.s === s) await marginChangePersona(persona, false);
  if (!wanted || marginPage.s !== s || marginPage.mode === wanted) return;
  try {
    await api(`/api/notes/${enc(note.id)}/margin`, { method: 'POST', body: { mode: wanted } });
    if (marginPage.s !== s) return;
    marginPage.mode = wanted;
    marginControl();
  } catch (e) {
    if (e instanceof Locked) throw e;
    if (marginPage.s === s) marginSetStatus('Could not save this mode. Try again.');
  }
}

function marginSetStatus(text, brief = false) {
  clearTimeout(marginPage.statusTimer);
  const token = {};
  const s = marginPage.s;
  marginPage.status = text;
  marginPage.statusToken = token;
  marginPage.brief = Boolean(brief && text);
  const status = $('#margin-status');
  if (status) {
    status.textContent = text;
    if (noAi(text)) status.insertAdjacentHTML('beforeend', ' <a href="#/settings/writing">Set up writing AI</a>');
  }
  if (!marginPage.brief) return;
  marginPage.statusTimer = setTimeout(() => {
    if (marginPage.s === s && marginPage.statusToken === token && marginPage.status === text) marginSetStatus('');
  }, NO_COMMENT_MS);
}

function marginAskKeys() {
  return shortcutText(shortcutKeys('ask'));
}

function marginMenuSlide(menu, open, button, key) {
  let run = marginPage[key];
  menu.inert = !open;
  if (stillMotion() || typeof menu.animate !== 'function') {
    run?.cancel();
    marginPage[key] = null;
    menu.hidden = !open;
    return;
  }
  if (!run) {
    if (open === !menu.hidden) return;
    menu.hidden = false;
    menu.style.transformOrigin = popEdge(menu, button).origin;
    run = menu.animate(MENU_SLIDE, { duration: MENU_MS, easing: EASE_OUT, fill: 'both' });
    run.pause();
    run.currentTime = open ? 0 : MENU_MS;
    marginPage[key] = run;
    run.onfinish = () => {
      const shown = run.playbackRate > 0;
      run.onfinish = null;
      run.cancel();
      if (marginPage[key] === run) marginPage[key] = null;
      menu.hidden = !shown;
    };
  }
  menu.hidden = false;
  run.playbackRate = open ? 1 : -1 / EXIT_SHARE;
  run.play();
}

function marginMenu(which, open, focus = false) {
  const menu = $(`#margin-${which}-menu`);
  const button = $(`#margin-${which}`);
  if (!menu || !button) return;
  button.setAttribute('aria-expanded', String(open));
  marginMenuSlide(menu, open, button, which === 'mode' ? 'slide' : 'personaSlide');
  if (focus) {
    if (open) menu.querySelector('[aria-checked="true"]')?.focus({ preventScroll: true });
    else button.focus({ preventScroll: true });
  }
}

function marginModeMenu(open, focus = false) {
  marginMenu('mode', open, focus);
}

function marginPersonaMenu(open, focus = false) {
  marginMenu('persona', open, focus);
}

function marginWireMenu(button, menu, item, toggle) {
  const wrap = button.parentElement;
  button.addEventListener('click', () => toggle(button.getAttribute('aria-expanded') !== 'true', true));
  wrap.addEventListener('keydown', (e) => {
    if (e.key === 'Tab' && !menu.hidden) {
      const controls = [...document.querySelectorAll('a[href], button:not(:disabled), input:not(:disabled), select:not(:disabled), textarea:not(:disabled), [contenteditable], [tabindex]:not([tabindex="-1"])')].filter(el => el.getClientRects().length && (el === button || !wrap.contains(el)) && !el.closest('[inert]'));
      const next = controls[controls.indexOf(button) + (e.shiftKey ? -1 : 1)];
      toggle(false);
      if (next) { e.preventDefault(); next.focus({ preventScroll: true }); }
    } else if (e.key === 'Escape' && !menu.hidden) {
      e.preventDefault();
      e.stopPropagation();
      toggle(false, true);
    } else if ((e.key === 'Enter' || e.key === ' ') && e.target.closest(item)) {
      e.preventDefault();
      e.stopPropagation();
      e.target.closest(item).click();
    } else if (['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(e.key)) {
      e.preventDefault();
      e.stopPropagation();
      const items = [...menu.querySelectorAll(item)];
      const at = items.indexOf(document.activeElement);
      toggle(true);
      const next = e.key === 'Home' ? 0 : e.key === 'End' ? items.length - 1 : (at + (e.key === 'ArrowUp' ? items.length - 1 : 1)) % items.length;
      items[next]?.focus({ preventScroll: true });
    }
  });
  wrap.addEventListener('focusout', (e) => {
    if (!menu.hidden && e.relatedTarget && !wrap.contains(e.relatedTarget)) toggle(false);
  });
}

function marginControl() {
  const s = marginPage.s;
  const button = $('#margin-mode');
  const menu = $('#margin-mode-menu');
  if (!s || !button || !menu) return;
  if (!button.dataset.ready) {
    button.dataset.ready = '1';
    marginWireMenu(button, menu, '[data-margin-mode]', marginModeMenu);
    menu.addEventListener('click', (e) => {
      const option = e.target.closest('[data-margin-mode]');
      if (option) marginChangeMode(option.dataset.marginMode).catch(fail);
    });
  }
  const along = marginPage.mode === 'along';
  button.disabled = marginPage.changing;
  button.dataset.mode = marginPage.mode;
  const icon = $('#margin-mode-icon');
  if (icon && icon.dataset.along !== String(along)) {
    const paint = () => { icon.innerHTML = along ? '<svg><use href="#leo-mark"/></svg>' : ICON.chat; icon.dataset.along = String(along); };
    if (icon.dataset.along) iconSwap(icon, paint);
    else paint();
  }
  $('#margin-mode-label').textContent = along ? 'Reads along' : 'When you ask';
  button.title = along ? 'Felix reads changes after a pause. The text goes to your writing AI.' : `Select a passage, then choose Ask Felix or press ${marginAskKeys()}.`;
  for (const option of menu.querySelectorAll('[data-margin-mode]')) {
    option.setAttribute('aria-checked', String(option.dataset.marginMode === marginPage.mode));
    option.disabled = marginPage.changing;
  }
  marginPersonaControl();
}

function marginPersonaControl() {
  const button = $('#margin-persona');
  const menu = $('#margin-persona-menu');
  if (!marginPage.s || !button || !menu) return;
  if (!button.dataset.ready) {
    button.dataset.ready = '1';
    marginWireMenu(button, menu, '[data-margin-persona]', marginPersonaMenu);
    menu.addEventListener('click', (e) => {
      const option = e.target.closest('[data-margin-persona]');
      if (!option) return;
      if (option.dataset.marginPersona === 'instructions') {
        marginPersonaMenu(false);
        if (!marginPage.s?.note.id) return toast('Felix can read this note once it has been saved.');
        return marginInstructions(true);
      }
      if (option.dataset.marginPersona !== 'new') return marginChangePersona(option.dataset.marginPersona).catch(fail);
      marginPersonaMenu(false);
      personaStartNew = marginPage.s?.note.id || '';
      location.hash = '#/settings/felix';
    });
  }
  const list = marginPrefs?.personas || [];
  const now = list.find((p) => p.id === marginPage.persona) || list[0];
  button.disabled = !now;
  if (!now) return;
  $('#margin-persona-icon').innerHTML = PERSONA_ICON[now.icon] || PERSONA_ICON.book;
  $('#margin-persona-label').textContent = `Felix as ${now.name}`;
  button.title = `${now.name} reads this note. Shapes margin notes and replies.`;
  button.setAttribute('aria-label', `Who reads this note: ${now.name}`);
  const key = JSON.stringify([list, now.id]);
  if (menu.dataset.key === key) return;
  menu.dataset.key = key;
  menu.innerHTML = list.map((p) => `<button type="button" role="menuitemradio" aria-checked="${p.id === now.id}" data-margin-persona="${esc(p.id)}" title="${esc(p.details)}"><span class="persona-icon" aria-hidden="true">${PERSONA_ICON[p.icon] || PERSONA_ICON.book}</span><span class="persona-words"><b>${esc(p.name)}</b><small>${esc(p.line || p.details)}</small></span></button>`).join('')
    + `<hr><button type="button" role="menuitem" data-margin-persona="new"><span class="persona-icon" aria-hidden="true">${PERSONA_ICON.plus}</span><span class="persona-words"><b>New persona</b></span></button>`
    + `<button type="button" role="menuitem" data-margin-persona="instructions"><span class="persona-icon" aria-hidden="true">${PERSONA_ICON.pen}</span><span class="persona-words"><b>Instructions for this note…</b></span></button>`;
}

function updateReadHint(area) {
  const hint = area?.closest?.('[data-read-field]')?.querySelector('[data-read-hint]')
    || (area?.id && document.getElementById(`${area.id}-hint`));
  if (hint) hint.hidden = Array.from(area.value.trim()).length <= Number(area.dataset.reads);
}

function marginInstructions(open = false) {
  const box = $('#margin-instructions');
  const s = marginPage.s;
  if (!box || !s) return;
  const input = $('input', box);
  if (!box.dataset.ready) {
    box.dataset.ready = '1';
    input.addEventListener('keydown', (e) => {
      if (e.key !== 'Enter' && e.key !== 'Escape') return;
      e.preventDefault();
      e.stopPropagation();
      if (e.key === 'Escape') input.value = marginPage.instructions;
      input.blur();
    });
    input.addEventListener('blur', () => marginSaveInstructions(s, box, input).catch(fail));
  }
  if (document.activeElement === input) return;
  input.value = marginPage.instructions;
  updateReadHint(input);
  box.hidden = !open && !marginPage.instructions;
  if (open) input.focus({ preventScroll: true });
}

async function marginSaveInstructions(s, box, input) {
  const text = input.value.trim();
  if (marginPage.s !== s) return;
  if (text !== marginPage.instructions) {
    try {
      const done = await api(`/api/notes/${enc(s.note.id)}/margin`, { method: 'POST', body: { instructions: text } });
      if (marginPage.s !== s) return;
      marginPage.instructions = done.instructions ?? text;
      marginSetStatus(marginPage.instructions ? 'Instructions saved.' : 'Instructions removed.', true);
    } catch (e) {
      if (marginPage.s !== s) return;
      if (e instanceof Locked) throw e;
      return marginSetStatus('Could not save these instructions. Try again.');
    }
  }
  if (document.activeElement === input) return;
  input.value = marginPage.instructions;
  updateReadHint(input);
  box.hidden = !marginPage.instructions;
}

async function marginChangePersona(id, picked = true) {
  const s = marginPage.s;
  if (picked) marginPersonaMenu(false, true);
  if (!s) return;
  const before = marginPage.saved ?? marginPage.persona;
  const token = {};
  marginPage.personaToken = token;
  marginPage.persona = id;
  marginPersonaControl();
  if (!s.note.id) {
    marginPage.wantedPersona = id;
    return;
  }
  try {
    await api(`/api/notes/${enc(s.note.id)}/margin`, { method: 'POST', body: { persona: id } });
    if (marginPage.s === s && marginPage.personaToken === token) marginPage.saved = id;
  } catch (e) {
    if (marginPage.s !== s) return;
    if (e instanceof Locked) throw e;
    if (marginPage.personaToken !== token) return;
    marginPage.persona = before;
    marginPersonaControl();
    marginSetStatus('Could not save this persona. Try again.');
  }
}

async function marginChangeMode(mode) {
  const s = marginPage.s;
  if (!s || marginPage.changing || !['ask', 'along'].includes(mode)) return;
  marginModeMenu(false, true);
  if (mode === marginPage.mode) return;
  if (!s.note.id) {
    marginPage.wanted = mode;
    marginPage.mode = mode;
    marginPage.baseline = marginKit.blocksOf(s.doc.source());
    marginControl();
    return;
  }
  const before = { mode: marginPage.mode, baseline: marginPage.baseline };
  marginPage.mode = mode;
  marginPage.baseline = marginKit.blocksOf(s.doc.source());
  marginPage.changing = true;
  marginControl();
  try {
    const done = await api(`/api/notes/${enc(s.note.id)}/margin`, { method: 'POST', body: { mode } });
    if (marginPage.s !== s) return;
    marginPage.mode = done.mode;
    marginPage.baseline = marginKit.blocksOf(s.doc.source());
    clearTimeout(marginPage.timer);
    clearTimeout(marginPage.faceTimer);
    marginSetStatus('');
    marginRedraw();
  } catch (e) {
    if (marginPage.s !== s) return;
    Object.assign(marginPage, before);
    if (e instanceof Locked) throw e;
    marginSetStatus('Could not save this mode. Try again.');
  } finally {
    if (marginPage.s === s) {
      marginPage.changing = false;
      marginControl();
      if (document.activeElement === document.body) $('#margin-mode')?.focus({ preventScroll: true });
    }
  }
}

function marginRedraw() {
  if (!marginPage.layer) return;
  cancelAnimationFrame(marginPage.frame);
  marginPage.frame = requestAnimationFrame(marginDraw);
}

let marginFonts = 0;
function marginFontsIn() {
  marginFonts++;
  marginRedraw();
}

if (document.fonts) {
  Promise.all(['500 1em "Instrument Sans"', 'italic 1em "Instrument Sans"', '1em Newsreader', 'italic 1em Newsreader'].map((face) => document.fonts.load(face))).then(marginFontsIn, () => {});
  document.fonts.addEventListener('loading', () => document.fonts.ready.then(marginFontsIn));
}

function marginPrintable() {
  const s = marginPage.s;
  if (!s) return [];
  const source = s.doc.source();
  return marginPage.threads.map((t) => ({ at: marginKit.find(source, t), text: marginNow(t) })).filter(({ at, text }) => at && text).sort((a, b) => a.at.to - b.at.to);
}

function marginNow(t) {
  const felixSaid = [...(t.messages || [])].reverse().find((m) => m.from === 'felix');
  return felixSaid ? felixSaid.text : '';
}

const INK_SKIP = 'pre, code, figure, svg, a, .katex, [class*="math"], [class*="diagram"], [class*="mermaid"]';

function inkNodes(root) {
  const nodes = [];
  const walk = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  for (let node = walk.nextNode(); node; node = walk.nextNode()) {
    if (node.data.trim() && !node.parentElement.closest(INK_SKIP)) nodes.push(node);
  }
  const count = nodes.reduce((sum, node) => sum + node.data.split(/\s+/).filter(Boolean).length, 0);
  let at = 0;
  for (const node of nodes) {
    const pieces = document.createDocumentFragment();
    for (const part of node.data.split(/(\s+)/)) {
      if (!part.trim()) {
        pieces.append(part);
        continue;
      }
      const span = document.createElement('span');
      span.className = 'ink-word';
      span.style.animationDelay = `${marginKit.wordDelay(at++, count)}ms`;
      span.textContent = part;
      pieces.append(span);
    }
    node.replaceWith(pieces);
  }
}

function marginAnswer(line, text, arriving = false) {
  const md = window.leoMarkdown;
  const html = md.render(text, { interactive: false, rows: true, dir: marginPage.s?.note.directory || '' });
  const shape = document.createElement('div');
  shape.innerHTML = html;
  const only = shape.firstElementChild;
  if (shape.childElementCount !== 1 || only.tagName !== 'P' || only.childElementCount) {
    line.classList.add('prose');
    line.innerHTML = html;
    if (arriving) for (const fig of line.querySelectorAll('figure.drawing')) fig.dataset.play = '';
  } else {
    line.textContent = text;
  }
  if (!arriving) return;
  if (!stillMotion()) inkNodes(line);
  const said = line.parentElement;
  if (said?.classList.contains('margin-said')) said.scrollTop = Math.max(0, line.offsetTop - 28);
}

function marginElement(t) {
  const el = document.createElement('article');
  const more = `margin-more-${t.id}`;
  el.className = 'margin-note';
  el.dataset.id = t.id;
  el.setAttribute('aria-label', `Felix, about “${t.quote.slice(0, 80)}”`);
  el.innerHTML = `<button type="button" class="margin-toggle" aria-expanded="false" aria-controls="${esc(more)}"></button><button type="button" class="margin-menu" data-margin-act="menu" aria-haspopup="menu" aria-expanded="false" aria-label="Note options">${ICON.more}</button><div class="margin-said"></div><p class="margin-changed" hidden>The passage changed since Felix wrote this.</p><div class="margin-pending" hidden></div><div class="margin-more" id="${esc(more)}" aria-hidden="true" inert><div class="margin-more-body" data-read-field>
    <form class="margin-reply"><textarea rows="1" data-reads="${MARGIN_MESSAGE_READS}" aria-label="Reply to Felix" placeholder="Reply"></textarea><button type="submit" class="margin-send" aria-label="Send" disabled>${ICON.send}</button></form>
    <p class="margin-hint" data-read-hint role="status" hidden>Felix reads the first ${MARGIN_MESSAGE_READS.toLocaleString('en')} characters of this.</p>
    <p class="margin-hint" data-passage-hint hidden>Felix reads the first ${MARGIN_PASSAGE_READS.toLocaleString('en')} characters of the selected passage.</p>
    <p class="margin-hint" data-draft-hint hidden>Enter to ask · Esc to close</p>
    <p class="margin-problem" role="status" hidden></p>
    <div class="margin-acts"><button type="button" class="msg-act" data-margin-act="resolve">Resolve</button><button type="button" class="msg-act" data-margin-act="dismiss">Dismiss</button><button type="button" class="margin-add" data-margin-act="add">Add to note</button><button type="button" class="msg-act icon-act" data-action="diagram-expand" data-reply="all" aria-label="See it larger">${ICON.expand}</button><span class="margin-tools"><button type="button" class="margin-copy" data-margin-act="copy" aria-label="Copy"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="8.5" y="8.5" width="11" height="11" rx="2"/><path d="M15.5 8.5V6a1.5 1.5 0 0 0-1.5-1.5H6A1.5 1.5 0 0 0 4.5 6v8A1.5 1.5 0 0 0 6 15.5h2.5"/></svg></button><button type="button" class="margin-pin" data-margin-act="pin" aria-pressed="false">${ICON.pin}</button><button type="button" class="margin-chat" data-margin-act="chat" aria-label="Ask in chat">${ICON.chat}</button></span></div></div></div>`;
  marginPage.layer.appendChild(el);
  marginPage.els.set(t.id, el);
  marginPage.observer?.observe(el);
  return el;
}

const MARGIN_DRAWS = /\b(draw|sketch|visuali[sz]e|diagram|picture)/i;

function marginChanged(s) {
  const source = s.doc.source();
  return marginPage.threads.flatMap((t) => {
    const at = marginKit.find(source, t);
    return at?.how === 'context' ? [{ id: t.id, text: source.slice(at.from, at.to) }] : [];
  });
}

function marginPinnedOpen(t) {
  return Boolean(t.pinned) && t.id !== 'draft' && !marginPage.faded.has(t.id);
}

function marginIsOpen(t) {
  return marginPage.open === t.id || marginPinnedOpen(t) || marginPage.folds.has(t.id);
}

function marginFill(el, t, arriving, changed) {
  const said = $('.margin-said', el);
  const open = marginIsOpen(t);
  const resolved = marginPage.faded.has(t.id);
  const key = `${open}:${resolved}:${(t.messages || []).length}:${el.dataset.all || ''}`;
  if (el.dataset.drawn !== key) {
    el.dataset.drawn = key;
    said.textContent = '';
    if (!open) {
      if (resolved) said.insertAdjacentHTML('beforeend', '<div class="margin-resolved-label">Resolved</div>');
      const line = document.createElement('div');
      line.className = 'margin-felix';
      marginAnswer(line, (t.messages || []).find((m) => m.from === 'felix')?.text || '', arriving);
      said.append(line);
      const more = (t.messages || []).length - 1;
      if (more > 0) said.insertAdjacentHTML('beforeend', `<div class="margin-foot"><span>${more} ${more === 1 ? 'reply' : 'replies'}</span></div>`);
    } else {
      const all = t.messages || [];
      const folded = el.dataset.all ? 0 : Math.max(0, all.length - SHOWN_MESSAGES);
      if (folded) said.insertAdjacentHTML('beforeend', `<button type="button" class="margin-earlier" data-margin-act="earlier">Show ${folded} earlier</button>`);
      for (const message of all.slice(folded)) {
        const line = document.createElement('div');
        line.className = message.from === 'felix' ? 'margin-felix' : 'margin-you';
        if (message.from === 'felix') marginAnswer(line, message.text);
        else line.textContent = message.text;
        said.append(line);
      }
      if (all.length >= LONG_THREAD && marginNow(t)) said.insertAdjacentHTML('beforeend', '<p class="margin-long">This has turned into its own question. <button type="button" class="link-btn" data-margin-act="chat">Continue in chat</button> or <button type="button" class="link-btn" data-margin-act="add">make it a note</button>.</p>');
    }
  }
  $('.margin-changed', el).hidden = !changed;
  const draft = t.id === 'draft';
  const waiting = marginPage.thinking.get(t.id);
  const pending = $('.margin-pending', el);
  const asking = `${waiting ? 1 : 0}${waiting?.text || ''}`;
  if (pending.dataset.asking !== asking) {
    pending.dataset.asking = asking;
    pending.textContent = '';
    if (waiting) {
      if (waiting.text) {
        const you = document.createElement('div');
        you.className = 'margin-you';
        you.textContent = waiting.text;
        pending.append(you);
      }
      const sketching = MARGIN_DRAWS.test(waiting.text || '');
      pending.insertAdjacentHTML('beforeend', `<div class="margin-thinking" role="status" aria-label="${sketching ? 'Felix is sketching' : 'Felix is thinking'}">${sketching ? '<span class="margin-sketching" aria-hidden="true">Sketching it…</span>' : ''}<svg class="margin-drop" aria-hidden="true"><use href="#leo-mark"/></svg></div>`);
    }
  }
  pending.hidden = !waiting || !open;
  const more = $('.margin-more', el);
  if (open && !el.hasAttribute('data-open')) more.getBoundingClientRect();
  more.inert = !open;
  more.setAttribute('aria-hidden', String(!open));
  el.toggleAttribute('data-open', open);
  if (resolved) el.dataset.resolved = t.id;
  else delete el.dataset.resolved;
  const toggle = $('.margin-toggle', el);
  toggle.setAttribute('aria-expanded', String(open));
  toggle.setAttribute('aria-label', resolved ? `Reopen Felix’s resolved note on “${t.quote.slice(0, 80)}”` : `${open ? 'Close' : 'Open'} Felix’s note on “${t.quote.slice(0, 80)}”`);
  const pin = $('.margin-pin', el);
  pin.hidden = draft;
  $('.margin-menu', el).hidden = draft || resolved || open || Boolean(waiting);
  pin.setAttribute('aria-pressed', String(Boolean(t.pinned)));
  pin.setAttribute('aria-label', t.pinned ? 'Unpin' : 'Pin open');
  el.toggleAttribute('data-pinned', Boolean(t.pinned));
  el.toggleAttribute('data-newest', marginPage.newest === t.id);
  $('.margin-add', el).hidden = draft || !marginNow(t);
  $('[data-margin-act="copy"]', el).hidden = draft || !marginNow(t);
  $('[data-draft-hint]', el).hidden = !draft || Boolean(waiting);
  $('[data-passage-hint]', el).hidden = Array.from(t.quote).length <= MARGIN_PASSAGE_READS;
  $('[data-margin-act="resolve"]', el).hidden = draft;
  const dismiss = $('[data-margin-act="dismiss"]', el);
  dismiss.textContent = draft ? 'Cancel' : 'Dismiss';
  dismiss.hidden = draft && Boolean(waiting);
  const area = $('textarea', el);
  area.placeholder = draft ? 'Ask Felix about this…' : 'Reply';
  area.setAttribute('aria-label', draft ? 'Ask Felix about this passage' : 'Reply to Felix');
  if (waiting) el.setAttribute('aria-busy', 'true');
  else el.removeAttribute('aria-busy');
  $('.margin-send', el).disabled = Boolean(waiting) || !area.value.trim();
  el.toggleAttribute('data-active', marginLit(t.id));
}

const MARGIN_CUT_LINES = 12;

function marginCuts(boxes) {
  const notes = boxes.filter(({ el }) => el && !el.hasAttribute('data-open')).map(({ t, el }) => {
    const blocks = [...el.querySelectorAll('.margin-said > .margin-felix:not(.prose), .margin-said > .margin-felix.prose > :is(p, ul, ol, blockquote)')];
    return { id: t.id, el, blocks, line: blocks.length ? parseFloat(getComputedStyle(blocks[0]).lineHeight) : NaN, heights: blocks.map((block) => block.scrollHeight) };
  });
  for (const note of notes) note.cut = marginCut(note, MARGIN_CUT_LINES * note.line);
  return notes.filter((note) => note.cut);
}

function marginCut({ blocks, line, heights }, room) {
  let last = null;
  let cut = false;
  blocks.forEach((block, i) => {
    const fits = !(line > 0) || heights[i] <= room + 1;
    const allowed = fits ? heights[i] : room >= line ? Math.floor(room / line + 0.001) * line : 0;
    if (fits) block.style.removeProperty('--room');
    else block.style.setProperty('--room', `${allowed}px`);
    block.classList.toggle('margin-cut', !fits && allowed > 0);
    block.classList.toggle('margin-over', !fits && allowed === 0);
    if (!fits && allowed === 0) last?.classList.add('margin-cut');
    if (fits || allowed > 0) last = block;
    if (!fits) cut = true;
    room = fits ? room - heights[i] : 0;
  });
  return cut;
}

function marginGrow(cuts, items, spare) {
  const grown = cuts.filter(({ id, line }) => spare[id] >= line);
  for (const note of grown) marginCut(note, MARGIN_CUT_LINES * note.line + spare[note.id]);
  const heights = grown.map(({ el }) => el.offsetHeight);
  grown.forEach((note, i) => {
    const item = items.find(({ id }) => id === note.id);
    const over = heights[i] - item.height - spare[note.id];
    if (over > 0) marginCut(note, MARGIN_CUT_LINES * note.line + spare[note.id] - Math.ceil(over / note.line) * note.line);
    item.height = heights[i] - Math.max(0, over);
  });
}

function marginLit(id) {
  return marginPage.open === id || marginPage.hover === id || marginPage.focus === id;
}

function marginPath(id, kind, line = 0) {
  const key = `${id}:${kind}:${line}`;
  let path = marginPage.paths.get(key);
  if (!path) {
    path = document.createElementNS('http://www.w3.org/2000/svg', 'path');
    path.setAttribute('class', kind);
    path.dataset.id = id;
    path.dataset.line = String(line);
    marginPage.svg.appendChild(path);
    marginPage.paths.set(key, path);
  }
  return path;
}

function marginThreadPaths(id) {
  return [...marginPage.paths].filter(([key]) => key.startsWith(`${id}:`));
}

function marginDropThread(id) {
  const el = marginPage.els.get(id);
  if (el) marginPage.observer?.unobserve(el);
  el?.remove();
  marginPage.els.delete(id);
  marginPage.marks.get(id)?.remove();
  marginPage.marks.delete(id);
  marginPage.layer?.parentElement.querySelector(`.margin-slot[data-id="${CSS.escape(id)}"]`)?.remove();
  marginPage.rects.delete(id);
  for (const [key, path] of marginThreadPaths(id)) {
    path.remove();
    marginPage.paths.delete(key);
  }
  if (marginPage.open === id) marginPage.open = null;
  if (marginPage.hover === id) marginPage.hover = null;
  if (marginPage.focus === id) marginPage.focus = null;
}

const marginReplies = (t) => Math.max(0, (t.messages || []).length - 1);

function marginMark(t) {
  let mark = marginPage.marks.get(t.id);
  if (!mark) {
    mark = document.createElement('button');
    mark.type = 'button';
    mark.className = 'margin-mark';
    mark.dataset.id = t.id;
    mark.setAttribute('aria-label', `Felix’s note on “${t.quote.slice(0, 80)}”`);
    mark.innerHTML = t.id === 'reading' ? '<svg class="margin-drop" aria-hidden="true"><use href="#leo-mark"/></svg>' : '<span class="margin-anchor" aria-hidden="true"><svg><use href="#leo-mark"/></svg></span><span class="margin-count" aria-hidden="true"></span>';
    if (t.id === 'reading') {
      mark.classList.add('reading');
      mark.tabIndex = -1;
      mark.setAttribute('aria-hidden', 'true');
    }
    marginPage.layer.appendChild(mark);
    marginPage.marks.set(t.id, mark);
  }
  return mark;
}

function marginPaint() {
  for (const [id, d] of marginPage.tethers) {
    const rest = marginPage.threads.some((t) => t.id === id && t.pinned) || (marginPage.newest === id && !marginPage.faded.has(id));
    for (const part of ['tie', 'spine']) {
      const key = `${id}:tether ${part}:0`;
      if (d?.[part] && (marginLit(id) || rest)) {
        const fresh = !marginPage.paths.has(key);
        const path = marginPath(id, `tether ${part}`);
        path.setAttribute('d', d[part]);
        path.classList.toggle('rest', rest);
        if (fresh) {
          path.setAttribute('pathLength', '1');
          if (!stillMotion()) path.classList.add('sketching');
        }
      } else {
        marginPage.paths.get(key)?.remove();
        marginPage.paths.delete(key);
      }
    }
    if (!d) marginPage.tethers.delete(id);
  }
  for (const [key, path] of marginPage.paths) path.classList.toggle('on', marginLit(key.slice(0, key.indexOf(':'))));
  for (const [id, el] of marginPage.els) el.toggleAttribute('data-active', marginLit(id));
  for (const [id, mark] of marginPage.marks) mark.toggleAttribute('data-active', marginLit(id));
}

function marginInk(t, rects) {
  const id = t.id;
  const wash = marginPath(id, 'wash');
  wash.setAttribute('d', rects.map((r) => `M${(r.left - 2).toFixed(1)} ${(r.top + 1).toFixed(1)} H${(r.right + 2).toFixed(1)} V${(r.bottom + 3).toFixed(1)} H${(r.left - 2).toFixed(1)} Z`).join(' '));
  const arriving = marginPage.arriving.has(id) && !stillMotion();
  const total = rects.reduce((sum, r) => sum + Math.max(1, r.right - r.left), 0);
  let before = 0;
  rects.forEach((r, line) => {
    const dots = marginPath(id, 'squiggle', line);
    dots.setAttribute('d', `M${r.left.toFixed(1)} ${(r.bottom + 4).toFixed(1)} H${r.right.toFixed(1)}`);
    if (r.formula) dots.style.setProperty('--mark', `${Math.max(20, r.bottom - r.top).toFixed(1)}px`);
    else dots.style.removeProperty('--mark');
    const width = Math.max(1, r.right - r.left);
    if (arriving && !dots.classList.contains('arriving')) {
      dots.style.setProperty('--wipe-d', `${Math.round((WIPE_MS * before) / total)}ms`);
      dots.style.setProperty('--wipe-t', `${Math.max(1, Math.round((WIPE_MS * width) / total))}ms`);
      dots.classList.add('arriving');
    }
    before += width;
  });
  for (const [key, path] of marginThreadPaths(id)) {
    if (path.classList.contains('squiggle') && Number(path.dataset.line) >= rects.length) {
      path.remove();
      marginPage.paths.delete(key);
    }
  }
  const seed = marginSeed(id);
  marginPath(id, 'solid').setAttribute('d', rects.map((r, line) => marginWobble(r.left, r.bottom + 4, Math.max(1, r.right - r.left), false, seed + line, 2)).join(' '));
}

function marginDraw() {
  const { s, layer } = marginPage;
  if (!s || !layer) return;
  if (!layer.isConnected) return marginLeave();
  marginPage.tethers = new Map([...marginPage.tethers.keys()].map((id) => [id, '']));
  marginPage.markers = new Map();
  const columns = layer.parentElement;
  const source = s.doc.source();
  const faded = ['right', 'both'].includes(layer.parentElement.dataset.margin) ? [...marginPage.faded.values()] : [];
  const threads = [...marginPage.threads, ...faded, ...(marginPage.draft ? [marginPage.draft] : [])];
  const { placed, gone, edit } = marginPlaced(source, threads);
  marginPage.gone = gone;
  marginPage.moved = placed.filter(({ t, at }) => at.how === 'context' && marginPage.threads.includes(t) && marginKept(t));
  const layout = marginLayout(columns);
  if (columns.dataset.margin !== layout) {
    columns.dataset.margin = layout;
    if (!askFloat.hidden) placeAskFloat(true);
  }
  const margins = layout === 'right' || layout === 'both';
  const slots = marginSlots(margins ? [] : placed, columns);
  const keep = new Set([...placed.map(({ t }) => t.id), 'reading']);
  for (const id of [...marginPage.els.keys(), ...marginPage.marks.keys()]) if (!keep.has(id)) marginDropThread(id);
  const origin = columns.getBoundingClientRect();
  const page = $('article.note', columns).getBoundingClientRect();
  const folds = new Map();
  const boxes = placed.map(({ t, at }) => ({ t, at, rects: s.doc.spanRects(at.from, at.to).map((r) => ({ ...r, left: r.left - origin.left, right: r.right - origin.left, top: r.top - origin.top, bottom: r.bottom - origin.top })) })).filter(({ t, at, rects }) => {
    const head = s.doc.foldedHeading(at.from);
    if (head !== null) folds.set(head, (folds.get(head) || 0) + (t.status === 'resolved' ? 0 : 1));
    return rects.length && head === null;
  }).map((box) => {
    const el = marginPage.els.get(box.t.id) || (margins || marginIsOpen(box.t) ? marginElement(box.t) : null);
    if (el) marginFill(el, box.t, marginPage.arriving.has(box.t.id), box.at.how !== 'exact' || box.t.reworded);
    return { ...box, el };
  });
  s.doc.foldNotes(folds);
  const cuts = marginCuts(boxes);
  const shown = new Set(boxes.map(({ t }) => t.id));
  for (const [id, el] of marginPage.els) if (!shown.has(id)) el.hidden = true;
  for (const { t } of placed) {
    if (shown.has(t.id)) continue;
    marginPage.marks.get(t.id)?.remove();
    marginPage.marks.delete(t.id);
    marginPage.rects.delete(t.id);
    for (const [key, path] of marginThreadPaths(t.id)) {
      path.remove();
      marginPage.paths.delete(key);
    }
  }
  const reading = marginReadingBox(origin);
  if (reading) boxes.push(reading);
  marginPencil(origin);
  for (const block of marginPage.roomy.values()) block.classList.remove('margin-room');
  marginPage.roomy = new Map();
  if (margins) marginPlaceInMargin(boxes, layout, page, origin, cuts);
  else {
    marginPage.sides = {};
    columns.style.paddingBottom = '';
    marginPlaceAsMarks(boxes, page, origin, slots);
  }
  for (const { t, rects } of boxes) {
    if (t.id === 'reading') continue;
    marginPage.rects.set(t.id, rects);
    marginInk(t, rects);
  }
  marginRevisits(source, origin, placed.filter(({ t }) => marginPage.threads.includes(t)).map(({ at }) => at), edit);
  marginPaint();
  if (popup?.owner.matches('.margin-menu') && (popup.owner.hidden || !popup.owner.isConnected)) closePopup(false);
  inkSettle((id) => marginPage.els.get(id)?.querySelectorAll('.margin-felix') || []);
  if (marginPage.arriving.size && !stillMotion()) {
    const done = [...marginPage.arriving];
    setTimeout(() => {
      for (const id of done) {
        marginPage.arriving.delete(id);
        for (const [, path] of marginThreadPaths(id)) path.classList.remove('arriving');
        marginPage.els.get(id)?.classList.remove('arriving');
      }
    }, MARGIN_SETTLE_MS);
  } else marginPage.arriving.clear();
}

function marginFind(source, t, edit) {
  const key = `${t.quote}\u0000${t.prefix}\u0000${t.suffix}`;
  const cached = marginPage.finds.get(t.id);
  const now = marginPage.drawnAt;
  const usable = cached?.key === key && cached.at && (cached.drawnAt === now || (edit && cached.drawnAt === now - 1));
  const kept = usable ? marginKit.carried(source, cached.at, cached.drawnAt === now ? null : edit, cached.text, t) : null;
  const at = kept || marginKit.find(source, t);
  marginPage.finds.set(t.id, { key, at, text: at && source.slice(at.from, at.to), drawnAt: now });
  return at;
}

function marginIdle(fn) {
  if (window.requestIdleCallback) return window.requestIdleCallback(fn);
  return setTimeout(() => {
    const end = Date.now() + 8;
    fn({ timeRemaining: () => end - Date.now() });
  }, 0);
}

function marginSettle(s) {
  const source = marginPage.drawn;
  if (s !== marginPage.s || source === null) return;
  const left = [...marginPage.resolved, ...marginPage.faded.values(), ...(marginPage.draft ? [marginPage.draft] : []), ...marginPage.threads];
  const step = (deadline) => {
    if (s !== marginPage.s || marginPage.drawn !== source) return;
    do {
      const t = left.pop();
      if (!t) return marginRedraw();
      marginPage.finds.delete(t.id);
      marginFind(source, t, null);
    } while (deadline.timeRemaining() > 0);
    marginIdle(step);
  };
  marginIdle(step);
}

function marginPlaced(source, threads) {
  const edit = marginPage.drawn === source ? null : marginKit.edited(marginPage.drawn ?? '', source);
  if (edit) marginPage.drawnAt += 1;
  marginPage.drawn = source;
  const placed = [];
  const gone = [];
  for (const t of threads) {
    const at = marginFind(source, t, edit);
    if (at) placed.push({ t, at });
    else gone.push(t.id);
  }
  return { placed, gone, edit };
}

function marginRevisits(source, origin, open, edit) {
  const live = new Set();
  for (const r of marginPage.resolved) {
    const at = marginFind(source, r, edit);
    const covered = at && open.some((o) => o.from < at.to && at.from < o.to);
    const rects = at && !covered ? marginPage.s.doc.spanRects(at.from, at.to) : [];
    if (!rects.length) continue;
    live.add(r.id);
    const els = marginPage.revisits.get(r.id) || [];
    marginPage.revisits.set(r.id, els);
    rects.forEach((rect, line) => {
      let el = els[line];
      if (!el) {
        el = document.createElement(line ? 'span' : 'button');
        el.className = 'margin-revisit';
        el.dataset.resolved = r.id;
        if (line) el.setAttribute('aria-hidden', 'true');
        else {
          el.type = 'button';
          el.title = "Reopen Felix's note";
          el.setAttribute('aria-label', `Reopen Felix’s note on “${r.quote.slice(0, 80)}”`);
        }
        marginPage.layer.appendChild(el);
        els[line] = el;
      }
      el.style.transform = `translate(${Math.round(rect.left - origin.left)}px, ${Math.round(rect.bottom - origin.top)}px)`;
      el.style.width = `${Math.max(1, Math.round(rect.right - rect.left))}px`;
    });
    for (const extra of els.splice(rects.length)) extra.remove();
  }
  for (const [id, els] of marginPage.revisits) {
    if (live.has(id)) continue;
    for (const el of els) el.remove();
    marginPage.revisits.delete(id);
  }
}

async function marginReopen(id) {
  const s = marginPage.s;
  if (marginPage.reopening.has(id)) return;
  marginPage.reopening.add(id);
  try {
    const done = await api(`/api/notes/${enc(s.note.id)}/margin/threads/${enc(id)}`, { method: 'POST', body: { status: 'open', gone: marginPage.gone } });
    if (marginPage.s !== s) return;
    marginPage.resolved = marginPage.resolved.filter((r) => r.id !== id);
    marginPage.faded.delete(id);
    if (!marginPage.threads.some((t) => t.id === done.id)) marginPage.threads.push(done);
    marginOpen(done.id, true);
  } finally {
    marginPage.reopening.delete(id);
  }
}

function marginPlaceInMargin(boxes, layout, page, origin, cuts) {
  const left = page.left - origin.left;
  const right = page.right - origin.left;
  const size = parseFloat(getComputedStyle(marginPage.layer.parentElement).getPropertyValue('--margin-w')) || marginKit.MARGIN_WIDTH;
  const all = boxes.map(({ t, rects, el }) => ({ id: t.id, want: rects.length > 1 ? (rects[0].top + rects[rects.length - 1].bottom) / 2 - 10 : rects[0].top - 2, height: el.offsetHeight }));
  const items = all.filter((i) => i.id !== 'reading');
  const side = layout === 'both' ? marginKit.sides(items, 16, marginPage.sides) : Object.fromEntries(items.map((item) => [item.id, 'right']));
  marginPage.sides = layout === 'both' ? side : {};
  const pinned = marginPage.open;
  const stacks = ['right', 'left'].map((on) => items.filter((i) => side[i.id] === on));
  const tops = Object.assign({}, ...stacks.map((list) => marginKit.stack(list, { pinned })));
  marginGrow(cuts, items, Object.assign({}, ...stacks.map((list) => marginKit.spare(list, tops))));
  const end = Math.max(0, ...items.map((i) => tops[i.id] + i.height));
  const reach = end && end - (Math.max(...[...$('article.note', marginPage.layer.parentElement).children].map((el) => el.getBoundingClientRect().bottom)) - origin.top);
  marginPage.layer.parentElement.style.paddingBottom = reach > 0 ? `${Math.ceil(reach)}px` : '';
  const dots = all.find((i) => i.id === 'reading');
  if (dots) {
    const spot = marginKit.clear(dots, items.map((i) => ({ top: tops[i.id], height: i.height, side: side[i.id] })), layout === 'both');
    side.reading = spot.side;
    tops.reading = spot.top;
  }
  for (const { t, at, el, rects } of boxes) {
    const on = side[t.id];
    const x = on === 'right' ? right + marginKit.MARGIN_GAP : left - marginKit.MARGIN_GAP - size;
    el.hidden = false;
    el.removeAttribute('data-inline');
    if (el.dataset.side !== on) el.dataset.side = on;
    el.style.transform = `translate(${Math.round(x)}px, ${Math.round(tops[t.id])}px)`;
    el.style.width = '';
    el.classList.toggle('arriving', marginPage.arriving.has(t.id));
    marginPage.marks.get(t.id)?.remove();
    marginPage.marks.delete(t.id);
    if (t.id === 'reading') continue;
    const first = rects[0];
    const last = rects[rects.length - 1];
    const multi = rects.length > 1;
    const mid = multi ? (first.top + last.bottom) / 2 : (first.top + first.bottom) / 2;
    const from = on === 'right' ? (multi ? right : last.right) + 8 : marginLeftEdge(at, rects, origin);
    const to = on === 'right' ? x - 2 : x + size + 2;
    const top = tops[t.id] + 10;
    const bend = (from + to) / 2;
    marginPage.tethers.set(t.id, {
      tie: `M${from.toFixed(1)} ${mid.toFixed(1)} C${bend.toFixed(1)} ${mid.toFixed(1)} ${bend.toFixed(1)} ${top.toFixed(1)} ${to.toFixed(1)} ${top.toFixed(1)}`,
      spine: multi ? marginWobble(from, first.top, last.bottom - first.top, true, marginSeed(t.id), 2) : '',
    });
  }
}

function marginMarker(li) {
  const task = li.querySelector(':scope > .task > input');
  if (task) return task.getBoundingClientRect().left;
  const list = li.parentElement;
  let width = marginPage.markers.get(li);
  if (width === undefined) {
    const copy = list.cloneNode(false);
    const item = li.cloneNode(false);
    const probe = document.createElement('span');
    if (list.tagName === 'OL') item.setAttribute('value', li.hasAttribute('value') ? li.value : list.start + [...list.children].indexOf(li));
    copy.style.cssText = 'position: absolute; visibility: hidden; left: 0; top: 0; margin: 0; padding: 0; width: max-content';
    item.style.cssText = 'margin: 0; padding: 0; list-style-position: inside';
    probe.style.cssText = 'display: inline-block; width: 0; height: 0';
    item.append(probe);
    copy.append(item);
    list.parentElement.append(copy);
    width = probe.getBoundingClientRect().left - item.getBoundingClientRect().left;
    copy.remove();
    marginPage.markers.set(li, width);
  }
  return li.getBoundingClientRect().left - width;
}

function marginLeftEdge(at, rects, origin) {
  const doc = marginPage.s.doc;
  const first = doc.blockAt(at.from);
  const last = doc.blockAt(at.to) || first;
  const items = [];
  for (let block = first; block; block = block.nextElementSibling) {
    items.push(...block.querySelectorAll('li'));
    if (block === last) break;
  }
  const blockLeft = first ? first.getBoundingClientRect().left - origin.left : rects[0].left;
  return Math.min(...rects.map((r) => {
    const y = (r.top + r.bottom) / 2 + origin.top;
    const li = items.findLast((item) => { const b = item.getBoundingClientRect(); return y > b.top && y < b.bottom; });
    return li ? marginMarker(li) - origin.left - 3 : blockLeft - 6;
  }));
}

function marginPassageEnd(t, rects, origin) {
  const span = t.id === 'reading' ? marginPage.readingSpan : null;
  const got = span && marginPage.s.doc.spanRects(Math.max(span.from, span.to - 200), span.to).at(-1);
  if (!got) return rects[rects.length - 1];
  return { left: got.left - origin.left, right: got.right - origin.left, top: got.top - origin.top, bottom: got.bottom - origin.top };
}

function marginSlots(placed, columns) {
  const doc = marginPage.s.doc;
  const pointAt = (to) => {
    const point = doc.domPoint(to);
    const text = point?.node;
    return text?.nodeType === Node.TEXT_NODE && text.parentElement.closest('.blk') && !text.parentElement.closest('.line-edit') ? point : null;
  };
  const beside = (node, way) => {
    const run = [];
    for (let at = node[way]; at?.classList?.contains('margin-slot'); at = at[way]) run.push(at);
    return run;
  };
  const want = new Map(placed.map(({ t, at }) => [t.id, at.to]));
  const slots = new Map();
  for (const slot of columns.querySelectorAll('.margin-slot')) {
    const id = slot.dataset.id;
    const point = want.has(id) && pointAt(want.get(id));
    const there = point && ((point.offset === point.node.data.length && beside(point.node, 'nextSibling').includes(slot)) || (point.offset === 0 && beside(point.node, 'previousSibling').includes(slot)));
    if (there) {
      want.delete(id);
      slots.set(id, slot);
    } else slot.remove();
  }
  for (const [id, to] of want) {
    const point = pointAt(to);
    if (!point) continue;
    const slot = document.createElement('span');
    slot.className = 'margin-slot';
    slot.dataset.id = id;
    slot.setAttribute('aria-hidden', 'true');
    leoDoc.insertAt(point, slot);
    slots.set(id, slot);
  }
  for (const { t } of placed) slots.get(t.id)?.toggleAttribute('data-more', marginReplies(t) > 0);
  const blockOf = (slot) => slot.closest('.blk');
  const fit = (slot) => {
    const block = blockOf(slot);
    return `${block.clientWidth} ${block.offsetHeight} ${block.querySelectorAll('.margin-slot').length} ${block.querySelectorAll('.margin-slot[data-more]').length} ${marginFonts}`;
  };
  const stale = new Set([...slots.values()].filter((slot) => slot.dataset.fit !== fit(slot)).map(blockOf));
  const each = [...slots.values()].filter((slot) => stale.has(blockOf(slot)));
  const heights = () => each.map((slot) => blockOf(slot).offsetHeight);
  for (const slot of each) slot.hidden = true;
  const bare = heights();
  for (const slot of each) slot.hidden = false;
  heights().forEach((height, i) => { each[i].hidden = height > bare[i]; });
  for (const slot of each) slot.dataset.fit = fit(slot);
  return slots;
}

function marginPlaceAsMarks(boxes, page, origin, slots) {
  const columns = marginPage.layer.parentElement;
  const edge = columns.clientWidth;
  const taken = [];
  const used = new Map();
  for (const { t, at, el } of [...boxes].sort((a, b) => (a.at?.to ?? 0) - (b.at?.to ?? 0))) {
    if (!el) continue;
    const open = marginIsOpen(t);
    el.hidden = !open;
    if (!open) {
      delete el.dataset.unfolded;
      continue;
    }
    const block = marginPage.s.doc.blockAt(at.to);
    if (!block) continue;
    el.setAttribute('data-inline', '');
    el.removeAttribute('data-side');
    const x = Math.max(0, page.left - origin.left - 12);
    el.style.width = `${Math.min(page.width + 24, 560, edge - x)}px`;
    const above = used.get(block) ?? CARD_TOP;
    const below = above + el.offsetHeight + CARD_GAP;
    used.set(block, below);
    const room = below + CARD_TAIL;
    block.style.setProperty('--margin-room', `${room}px`);
    block.classList.add('margin-room');
    marginPage.roomy.set(t.id, block);
    el.style.transform = `translate(${Math.round(x)}px, ${Math.round(block.getBoundingClientRect().bottom - origin.top + above)}px)`;
    if (el.dataset.unfolded) continue;
    el.dataset.unfolded = '1';
    if (stillMotion() || marginPage.unfold !== t.id) continue;
    el.animate([{ clipPath: 'inset(0 0 100% 0)', opacity: .4 }, { clipPath: 'inset(0 0 0 0)', opacity: 1 }], { duration: OPEN_MS, easing: EASE_OUT });
    const grow = block.animate([{ marginBottom: `${above - CARD_TOP}px` }, { marginBottom: `${room}px` }], { duration: OPEN_MS, easing: EASE_OUT });
    marginPage.unfolds.add(t.id);
    grow.onfinish = grow.oncancel = () => {
      marginPage.unfolds.delete(t.id);
      marginRedraw();
    };
  }
  for (const { t, rects } of boxes) {
    const mark = marginMark(t);
    const replies = marginReplies(t);
    mark.toggleAttribute('data-more', replies > 0);
    const count = $('.margin-count', mark);
    if (count) {
      count.textContent = replies > 9 ? '9+' : replies || '';
      mark.setAttribute('aria-label', `Felix’s note on “${t.quote.slice(0, 80)}”${replies ? `, ${plural(replies, 'reply', 'replies')}` : ''}`);
    }
    const slot = slots.get(t.id)?.getBoundingClientRect();
    const end = marginPassageEnd(t, rects, origin);
    const room = page.right - origin.left;
    const inline = slot?.height && slot.right <= page.right;
    let aside = t.id !== 'reading' && !inline;
    mark.toggleAttribute('data-aside', aside);
    let size = mark.offsetWidth;
    let x, y;
    if (inline) {
      x = slot.left - origin.left + slot.width / 2 - size / 2;
      y = slot.top - origin.top + slot.height / 2 - mark.offsetHeight / 2;
    } else if (!aside) {
      x = end.right + 13 - size / 2;
      y = (end.top + end.bottom) / 2 - mark.offsetHeight / 2;
      while (taken.some((at) => Math.abs(at.x - x) < 14 && Math.abs(at.y - y) < 8)) x += 14;
      aside = x + size / 2 + 6 > room;
      mark.toggleAttribute('data-aside', aside);
      size = mark.offsetWidth;
    }
    if (aside) {
      x = room + 16 - size;
      y = (end.top + end.bottom) / 2 - mark.offsetHeight / 2;
      while (taken.some((at) => Math.abs(at.x - x) < size && Math.abs(at.y - y) < mark.offsetHeight)) y += mark.offsetHeight;
    }
    taken.push({ x, y });
    mark.style.transform = `translate(${Math.round(x)}px, ${Math.round(y)}px)`;
  }
}

function marginFoldAway(id, then) {
  const el = marginPage.els.get(id);
  if (!el || el.hidden || !el.hasAttribute('data-inline') || marginThread(id)?.pinned || stillMotion()) return then();
  if (marginPage.folds.has(id)) return;
  el.inert = true;
  const s = marginPage.s;
  const block = marginPage.roomy.get(id);
  const shared = [...marginPage.roomy].some(([other, at]) => other !== id && at === block);
  const shut = { duration: OPEN_MS * EXIT_SHARE, easing: EASE_EXIT, fill: 'forwards' };
  const runs = [el.animate([{ clipPath: 'inset(0 0 0 0)', opacity: 1 }, { clipPath: 'inset(0 0 100% 0)', opacity: 0 }], shut)];
  if (block) {
    const now = parseFloat(getComputedStyle(block).marginBottom) || 0;
    runs.push(block.animate([{ marginBottom: `${now}px` }, { marginBottom: `${shared ? Math.max(0, now - el.offsetHeight - CARD_GAP) : 0}px` }], shut));
  }
  const end = () => {
    if (marginPage.folds.get(id) === stop) marginPage.folds.delete(id);
    el.inert = false;
  };
  const stop = () => {
    end();
    for (const run of runs) run.cancel();
  };
  marginPage.folds.set(id, stop);
  runs[0].finished.then(() => {
    end();
    if (marginPage.s === s) then();
    stop();
  }, end);
}

function marginHover(e) {
  if (!marginPage.layer || e.pointerType === 'touch' || marginPage.pointer) return;
  const { clientX, clientY, target } = e;
  marginPage.pointer = requestAnimationFrame(() => {
    marginPage.pointer = 0;
    if (!marginPage.layer) return;
    const origin = marginPage.layer.parentElement.getBoundingClientRect();
    const x = clientX - origin.left;
    const y = clientY - origin.top;
    let id = target?.closest?.('.margin-note, .margin-mark')?.dataset.id || null;
    if (!id) {
      for (const [key, rects] of marginPage.rects) {
        if (rects.some((r) => x >= r.left - 2 && x <= r.right + 2 && y >= r.top && y <= r.bottom + 5)) {
          id = key;
          break;
        }
      }
    }
    if (id !== marginPage.hover) {
      marginPage.hover = id;
      marginPaint();
    }
  });
}

function marginFocus(e) {
  const id = e.type === 'focusin' ? e.target.closest?.('[data-id]')?.dataset.id || null : null;
  if (e.type === 'focusout' && e.relatedTarget && marginPage.layer?.contains(e.relatedTarget)) return;
  if (id === marginPage.focus) return;
  marginPage.focus = id;
  marginPaint();
}

document.addEventListener('pointermove', marginHover, { passive: true });
addEventListener('resize', marginRedraw);
document.documentElement.addEventListener('pointerleave', () => {
  if (!marginPage.hover) return;
  marginPage.hover = null;
  marginPaint();
});

function marginThread(id) {
  if (marginPage.draft?.id === id) return marginPage.draft;
  return marginPage.threads.find((t) => t.id === id) || null;
}

function marginOpen(id, focus = false) {
  marginPage.folds.get(id)?.();
  Object.assign(marginPage, { open: id, unfold: id });
  marginDraw();
  marginPage.unfold = null;
  if (focus) $('textarea', marginPage.els.get(id))?.focus({ preventScroll: true });
}

function marginClose(discard = false) {
  const id = marginPage.open;
  if (!id) return;
  marginFoldAway(id, () => {
    if (marginPage.open === id) marginPage.open = null;
    if (id === 'draft' && marginPage.open !== 'draft' && marginPage.draft) {
      if (discard) marginPage.drafted.delete(marginDraftKey(marginPage.draft));
      else marginStash();
      marginPage.draft = null;
    }
    marginDraw();
    if (!marginPage.open) (marginPage.marks.get(id) || $('.margin-toggle', marginPage.els.get(id)))?.focus();
  });
}

function marginClick(e) {
  if (e.target.closest('[data-step-go]')) return;
  const back = e.target.closest('[data-resolved]');
  if (back) {
    marginReopen(back.dataset.resolved).catch(fail);
    return;
  }
  const action = e.target.closest('[data-margin-act]');
  const id = e.target.closest('[data-id]')?.dataset.id;
  if (!id) return;
  if (action) {
    const t = marginThread(id);
    if (!t) return;
    if (action.dataset.marginAct === 'earlier') {
      marginPage.els.get(id).dataset.all = '1';
      marginDraw();
      $('.margin-reply textarea', marginPage.els.get(id)).focus({ preventScroll: true });
    } else if (action.dataset.marginAct === 'menu') marginActions(t, action);
    else if (action.dataset.marginAct === 'pin') marginPin(t).catch(fail);
    else if (action.dataset.marginAct === 'add') marginAdd(t).catch(fail);
    else if (action.dataset.marginAct === 'copy') marginCopy(t).catch(fail);
    else if (action.dataset.marginAct === 'chat') marginToChat(t);
    else if (action.dataset.marginAct === 'resolve') marginStatus(t, 'resolved').catch(fail);
    else if (t.id === 'draft') marginClose(true);
    else marginStatus(t, 'dismissed').catch(fail);
    return;
  }
  if (marginPage.open !== id || marginPage.folds.has(id)) marginOpen(id, true);
}

function marginActions(t, owner) {
  const said = Boolean(marginNow(t));
  const drawn = $('.margin-felix figure.diagram[data-drawn="yes"], .margin-felix figure.drawing:not(.pending)', marginPage.els.get(t.id));
  const now = (act) => () => {
    const live = marginThread(t.id);
    if (live) return act(live);
  };
  popupMenu(owner, {
    label: 'Felix’s note',
    rows: [
      ...(drawn ? [{ text: 'Open full screen', run: () => expandDiagrams(owner) }] : []),
      { text: 'Resolve', run: now((live) => marginStatus(live, 'resolved')) },
      ...(said ? [{ text: 'Copy', run: now(marginCopy) }] : []),
      { text: 'Pin open', run: now((live) => {
        const pinned = marginPin(live);
        $('.margin-toggle', marginPage.els.get(live.id))?.focus({ preventScroll: true });
        return pinned;
      }) },
      ...(said ? [{ text: 'Add to note', run: now((live) => {
        marginOpen(live.id, true);
        return marginAdd(live);
      }) }] : []),
      { line: true },
      { text: 'Dismiss', run: now((live) => marginStatus(live, 'dismissed')) },
    ],
  });
}

function marginKey(e) {
  if (e.key === 'Enter' && !e.shiftKey && e.target.matches('.margin-reply textarea')) {
    e.preventDefault();
    e.target.form.requestSubmit();
  }
}

document.addEventListener('keydown', (e) => {
  if (e.key !== 'Escape' || e.isComposing || !marginPage.open || !marginPage.layer?.isConnected) return;
  if (document.querySelector('.scrim, .popup-menu, .select-menu:not([hidden]), [role="menu"]:not([hidden]), [role="listbox"]:not([hidden])')) return;
  if (e.target.closest?.('input, textarea, select, [contenteditable]') && !e.target.closest('.margin-note')) return;
  e.preventDefault();
  e.stopPropagation();
  marginClose();
}, true);

document.addEventListener('pointerdown', (e) => {
  if (!marginPage.open || e.target.closest?.('.margin-note, .margin-mark, .popup-menu, .scrim, .scrim-gone')) return;
  if (marginPage.open === 'draft' && $('textarea', marginPage.els.get('draft'))?.value.trim()) return;
  const id = marginPage.open;
  marginFoldAway(id, () => {
    if (marginPage.open === id) marginPage.open = null;
    if (marginPage.open !== 'draft') {
      marginStash();
      marginPage.draft = null;
    }
    marginRedraw();
  });
});

function marginSaying(el, problem) {
  const box = $('.margin-problem', el);
  box.textContent = problem || '';
  box.hidden = !problem;
}

async function marginSubmit(e) {
  e.preventDefault();
  const el = e.target.closest('.margin-note');
  const area = $('textarea', el);
  const text = area.value.trim();
  const t = marginThread(el.dataset.id);
  const s = marginPage.s;
  if (!text || !t || !s || marginPage.thinking.has(t.id)) return;
  area.value = '';
  marginSized(area);
  if (t.id === 'draft') return marginAskOn(s, t, text);
  return marginReply(s, el, t, text, area);
}

async function marginReply(s, el, t, text, area = null) {
  marginSaying(el, '');
  marginPage.thinking.set(t.id, { text });
  marginDraw();
  if (t.reply?.text !== text) t.reply = { text, request: requestId() };
  try {
    const done = await api(`/api/notes/${enc(s.note.id)}/margin/threads/${enc(t.id)}/reply`, { method: 'POST', body: { text, request: t.reply.request } });
    if (marginPage.s !== s) return;
    marginPage.thinking.delete(t.id);
    marginPage.threads = marginPage.threads.map((old) => (old.id === done.id ? done : old));
    marginDraw();
    const fresh = marginPage.els.get(done.id);
    const replies = fresh && [...fresh.querySelectorAll('.margin-felix')];
    if (replies?.length && marginIsOpen(done)) marginAnswer(replies[replies.length - 1], marginNow(done), true);
    if (marginPage.open === done.id) $('textarea', fresh)?.focus();
  } catch (error) {
    if (marginPage.s !== s) return;
    marginPage.thinking.delete(t.id);
    marginDraw();
    if (error instanceof Locked) throw error;
    if (el.isConnected) {
      if (area) {
        area.value = text;
        marginSized(area);
      }
      marginSaying(el, error.status === 409 && error.data?.why === 'busy' ? 'Felix is still answering this.' : error.message);
    }
  }
}

function marginSized(area) {
  updateReadHint(area);
  area.style.height = 'auto';
  area.style.height = `${Math.min(area.scrollHeight, MARGIN_AREA_MAX)}px`;
  const send = area.form?.querySelector('.margin-send');
  if (send) send.disabled = !area.value.trim() || marginPage.thinking.has(area.closest('.margin-note')?.dataset.id);
}

function marginRekey(from, to) {
  const el = marginPage.els.get(from);
  marginPage.els.delete(from);
  if (el) {
    el.dataset.id = to;
    marginPage.els.set(to, el);
    const more = $('.margin-more', el);
    more.id = `margin-more-${to}`;
    $('.margin-toggle', el).setAttribute('aria-controls', more.id);
  }
  const mark = marginPage.marks.get(from);
  marginPage.marks.delete(from);
  if (mark) {
    mark.dataset.id = to;
    marginPage.marks.set(to, mark);
  }
  marginPage.rects.delete(from);
  for (const [key, path] of marginThreadPaths(from)) {
    marginPage.paths.delete(key);
    path.dataset.id = to;
    marginPage.paths.set(`${to}${key.slice(from.length)}`, path);
  }
  if (marginPage.hover === from) marginPage.hover = to;
  if (marginPage.focus === from) marginPage.focus = to;
}

async function marginAskOn(s, t, question) {
  marginPage.drafted.delete(marginDraftKey(t));
  marginSaying(marginPage.els.get('draft'), '');
  marginPage.thinking.set('draft', { text: question });
  marginDraw();
  if (t.ask?.question !== question) t.ask = { question, request: requestId() };
  let done;
  try {
    await flush(s);
    done = await api(`/api/notes/${enc(s.note.id)}/margin/threads`, { method: 'POST', body: { version: s.note.version, quote: t.quote, prefix: t.prefix, suffix: t.suffix, question, gone: marginPage.gone, request: t.ask.request } });
  } catch (error) {
    if (marginPage.s !== s) return;
    marginPage.thinking.delete('draft');
    if (error instanceof Locked) throw error;
    const el = marginPage.els.get('draft');
    if (marginPage.draft === t && marginPage.open === 'draft' && el) {
      const area = $('textarea', el);
      area.value = question;
      marginSized(area);
      marginSaying(el, error.message);
      marginDraw();
      area.focus({ preventScroll: true });
    } else {
      if (marginPage.draft === t) {
        marginDropThread('draft');
        marginPage.draft = null;
      }
      toast(error.message, { bad: true });
      marginDraw();
    }
    return;
  }
  if (marginPage.s !== s) return;
  marginPage.thinking.delete('draft');
  const open = marginPage.draft === t && marginPage.open === 'draft';
  if (marginPage.draft === t) {
    marginRekey('draft', done.id);
    marginPage.draft = null;
  }
  if (open) marginPage.open = done.id;
  else marginPage.arriving.add(done.id);
  marginPage.threads.push(done);
  marginDraw();
  if (!open) return;
  const fresh = marginPage.els.get(done.id);
  const replies = fresh && [...fresh.querySelectorAll('.margin-felix')];
  if (replies?.length) marginAnswer(replies[replies.length - 1], marginNow(done), true);
  $('textarea', fresh)?.focus();
}

const marginPins = new Map();

async function marginPin(t) {
  const s = marginPage.s;
  const pin = marginPins.get(t.id) || { latest: 0, saved: t.pinned };
  marginPins.set(t.id, pin);
  const mine = ++pin.latest;
  const show = (pinned) => {
    const live = marginThread(t.id);
    if (mine !== pin.latest || marginPage.s !== s || !live || live.pinned === pinned) return;
    live.pinned = pinned;
    marginDraw();
  };
  const pinned = !t.pinned;
  t.pinned = pinned;
  if (pinned || marginPage.open === t.id) marginDraw();
  else {
    const had = marginPage.els.get(t.id)?.contains(document.activeElement);
    marginFoldAway(t.id, () => {
      marginDraw();
      if (had) (marginPage.marks.get(t.id) || $('.margin-toggle', marginPage.els.get(t.id)))?.focus({ preventScroll: true });
    });
  }
  try {
    const done = await api(`/api/notes/${enc(s.note.id)}/margin/threads/${enc(t.id)}`, { method: 'PATCH', body: { pinned } });
    pin.saved = done.pinned;
    show(done.pinned);
  } catch (e) {
    show(pin.saved);
    throw e;
  } finally {
    if (mine === pin.latest) marginPins.delete(t.id);
  }
}

const marginLight = ({ id, quote, prefix, suffix }) => ({ id, quote, prefix, suffix });

async function marginStatus(t, status) {
  const s = marginPage.s;
  const at = marginPage.threads.indexOf(t);
  const wasOpen = marginPage.open === t.id;
  const box = marginPage.els.get(t.id);
  const typed = box ? $('.margin-reply textarea', box)?.value : '';
  marginPage.threads = marginPage.threads.filter((old) => old.id !== t.id);
  if (status === 'resolved') {
    marginPage.resolved.unshift(marginLight(t));
    marginPage.faded.set(t.id, t);
    if (wasOpen) marginPage.open = null;
  } else marginDropThread(t.id);
  marginDraw();
  try {
    await api(`/api/notes/${enc(s.note.id)}/margin/threads/${enc(t.id)}`, { method: 'POST', body: { status } });
  } catch (e) {
    if (marginPage.s === s && !marginThread(t.id)) {
      marginPage.resolved = marginPage.resolved.filter((old) => old.id !== t.id);
      marginPage.faded.delete(t.id);
      marginPage.threads.splice(Math.max(at, 0), 0, t);
      if (wasOpen) marginPage.open = t.id;
      marginDraw();
      const area = marginPage.els.get(t.id)?.querySelector('.margin-reply textarea');
      if (area && typed) {
        area.value = typed;
        marginSized(area);
      }
    }
    throw e;
  }
  if (marginPage.s !== s) return;
  if (marginThread(t.id)) {
    marginPage.threads = marginPage.threads.filter((old) => old.id !== t.id);
    marginDropThread(t.id);
    if (status === 'resolved' && !marginPage.resolved.some((old) => old.id === t.id)) marginPage.resolved.unshift(marginLight(t));
    marginDraw();
  }
  if (status === 'dismissed') {
    toast('Dismissed Felix’s note', {
      action: 'Undo',
      run: async () => {
        try {
          await api(`/api/notes/${enc(s.note.id)}/margin/threads/${enc(t.id)}`, { method: 'POST', body: { status: 'open', gone: marginPage.gone } });
        } catch (e) {
          if (e instanceof Locked) throw e;
          return toast(e.message, { bad: true });
        }
        if (marginPage.s === s) await marginLoad(s);
      },
    });
  }
}

async function marginCopy(t) {
  const said = (t.messages || []).filter((m) => m.from === 'felix').map((m) => m.text).join('\n\n');
  try {
    await navigator.clipboard.writeText(said);
  } catch (e) {
    return toast('Could not copy Felix’s note.', { bad: true });
  }
  toast('Copied Felix’s note');
}

function marginToChat(t) {
  const s = marginPage.s;
  const at = marginKit.find(s.doc.source(), t);
  const line = at ? marginKit.lineAfter(s.doc.source(), at.from) : 0;
  const title = cleanTitle(s.title.textContent) || s.note.title || 'Untitled';
  marginPage.open = null;
  marginPage.draft = null;
  chat.attachSelection({ note: s.note.id, title, from: line, to: line, text: t.quote });
  marginRedraw();
}

async function marginInsert(s, t, text, request) {
  await flush(s);
  const source = s.doc.source();
  const at = marginKit.find(source, t);
  const line = at ? marginKit.lineAfter(source, at.to) : null;
  try {
    return { done: await api(`/api/notes/${enc(s.note.id)}/insert`, { method: 'POST', body: { text, line, base: s.note.version, request } }), line };
  } catch (e) {
    if (e.status !== 409) throw e;
  }
  const fresh = await api(`/api/notes/${enc(s.note.id)}`);
  const moved = marginKit.find(fresh.body, t);
  const next = moved ? marginKit.lineAfter(fresh.body, moved.to) : null;
  return { done: await api(`/api/notes/${enc(s.note.id)}/insert`, { method: 'POST', body: { text, line: next, base: fresh.version, request } }), line: next };
}

async function marginWeave(s, t, request) {
  await flush(s);
  const path = `/api/notes/${enc(s.note.id)}/margin/threads/${enc(t.id)}/weave`;
  try {
    return await api(path, { method: 'POST', body: { base: s.note.version, request } });
  } catch (e) {
    if (e.status !== 409 || e.data?.why) throw e;
  }
  const fresh = await api(`/api/notes/${enc(s.note.id)}`);
  return api(path, { method: 'POST', body: { base: fresh.version, request } });
}

async function marginAdd(t) {
  if (marginPage.adding.has(t.id)) return;
  marginPage.adding.add(t.id);
  const s = marginPage.s;
  const el = marginPage.els.get(t.id);
  const button = $('.margin-add', el);
  button.disabled = true;
  button.textContent = 'Working it in…';
  marginSaying(el, '');
  t.addRequest = t.addRequest || requestId();
  let done, from = null, to = null, woven = true;
  try {
    done = await marginWeave(s, t, t.addRequest).catch((e) => {
      if (!['lost', 'code', 'long'].includes(e.data?.why)) throw e;
      woven = false;
      return null;
    });
    if (woven) ({ from, to } = done);
    else {
      const added = await marginInsert(s, t, marginNow(t), t.addRequest);
      done = added.done;
      if (added.line !== null) [from, to] = [added.line + 1, added.line + 3];
    }
  } catch (e) {
    if (e instanceof Locked) throw e;
    if (el.isConnected) marginSaying(el, e.status === 409 && !e.data?.why ? 'The note changed. Try once more.' : e.message);
    return;
  } finally {
    marginPage.adding.delete(t.id);
    button.disabled = false;
    button.textContent = 'Add to note';
  }
  const id = s.note.id;
  const threadPath = `/api/notes/${enc(id)}/margin/threads/${enc(t.id)}`;
  const open = () => state.view === 'note' && state.session && state.session.note.id === id;
  toast(woven ? 'Worked into the note' : from === null ? 'Its passage has moved, so this went at the end of the note' : 'Added to note', {
    action: 'Undo',
    run: async () => {
      try {
        await api(`/api/notes/${enc(id)}`, { method: 'PATCH', body: { body: done.before, base: done.version } });
        await api(threadPath, { method: 'POST', body: { status: 'open', gone: marginPage.gone } }).catch(() => {});
        if (open()) await showNote(id);
        toast('Took it out of the note');
      } catch (e) {
        toast(e.status === 409 ? 'That note has changed since, so leo left it as it is.' : e.message, { bad: true });
      }
    },
  });
  await api(threadPath, { method: 'POST', body: { status: 'resolved' } }).catch((e) => {
    if (e instanceof Locked) throw e;
  });
  if (open()) await showNote(id);
  if (open() && from !== null) {
    state.session.doc.glow(from, to);
    setTimeout(() => { if (open()) state.session.doc.glow(); }, GLOW_MS);
  }
}

function marginBeside(s, picked) {
  const size = [...picked.text.trim()].length;
  if (!marginPage.layer || chat.isOpen() || size < LEAST_ASKED) return null;
  return marginKit.around(s.doc.source(), picked.text, picked.from);
}

function marginSame(s, picked, beside) {
  const source = s.doc.source();
  const at = marginKit.find(source, { quote: picked.text, ...beside });
  return at && marginPage.threads.find((t) => {
    const it = marginKit.find(source, t);
    return it && it.from === at.from && it.to === at.to;
  });
}

function marginDraftKey({ quote, prefix, suffix }) {
  return `${quote}\u0000${prefix}\u0000${suffix}`;
}

function marginStash() {
  const draft = marginPage.draft;
  if (!draft) return;
  const key = marginDraftKey(draft);
  const text = $('textarea', marginPage.els.get('draft'))?.value || '';
  marginPage.drafted.delete(key);
  if (text.trim()) marginPage.drafted.set(key, text);
  if (marginPage.drafted.size > 20) marginPage.drafted.delete(marginPage.drafted.keys().next().value);
}

function marginTaken(s, picked) {
  const beside = marginBeside(s, picked);
  return Boolean(beside && marginSame(s, picked, beside));
}

function marginAskFirst(s, picked) {
  const beside = marginBeside(s, picked);
  if (!beside) return false;
  const same = marginSame(s, picked, beside);
  if (same) {
    marginOpen(same.id, true);
    return true;
  }
  const want = { quote: picked.text, prefix: beside.prefix, suffix: beside.suffix };
  if (marginPage.draft && marginDraftKey(marginPage.draft) === marginDraftKey(want)) {
    marginOpen('draft', true);
    return true;
  }
  if (marginPage.thinking.has('draft')) {
    toast('Felix is still answering the last one.');
    return true;
  }
  marginStash();
  marginDropThread('draft');
  marginPage.draft = { id: 'draft', ...want, messages: [] };
  marginOpen('draft', true);
  const area = $('textarea', marginPage.els.get('draft'));
  if (!area) return false;
  const kept = marginPage.drafted.get(marginDraftKey(want));
  if (kept) {
    area.value = kept;
    marginSized(area);
  }
  return true;
}

function marginAsk(s, picked, question = '') {
  const beside = marginBeside(s, picked);
  if (!beside) return false;
  const same = marginSame(s, picked, beside);
  if (same) {
    marginOpen(same.id, true);
    const el = marginPage.els.get(same.id);
    if (!question || question === ASK_PRESETS.this || !el) return true;
    if (marginPage.thinking.has(same.id)) toast('Felix is still answering this.');
    else marginReply(s, el, same, question).catch(fail);
    return true;
  }
  if (marginPage.thinking.has('draft')) {
    toast('Felix is still answering the last one.');
    return true;
  }
  marginStash();
  marginDropThread('draft');
  const draft = { id: 'draft', quote: picked.text, prefix: beside.prefix, suffix: beside.suffix, messages: [] };
  marginPage.draft = draft;
  marginOpen('draft', true);
  if (!marginPage.els.get('draft')) return false;
  marginAskOn(s, draft, question).catch(fail);
  return true;
}

function marginPlain() {
  return READ_PAUSE[marginPrefs?.pace] || READ_PAUSE.regular;
}

function marginDue(s, again) {
  const last = marginKit.changedBlocks(marginPage.baseline, s.doc.source()).pop();
  const more = marginKit.pauseFor(last, marginPrefs?.pace) - marginPlain();
  if (again && more > 0) marginPage.timer = setTimeout(() => marginDue(s, false), more);
  else marginRead(s).catch(fail);
}

function marginArm(s) {
  clearTimeout(marginPage.timer);
  marginPage.timer = setTimeout(() => marginDue(s, true), Math.max(marginPlain(), marginPage.nextRead - Date.now()));
}

function marginEdited(s) {
  if (s !== marginPage.s) return;
  clearTimeout(marginPage.faceTimer);
  if (marginPage.readingAt !== null || marginPage.readingEl) marginReadingDone(true);
  marginRedraw();
  if (marginPage.brief) marginSetStatus('');
  if (s.note.id && !marginPage.loaded) marginLoad(s).catch(() => {});
  clearTimeout(marginPage.anchorTimer);
  marginPage.anchorTimer = setTimeout(() => marginReanchor(s).catch(() => {}), REANCHOR_MS);
  clearTimeout(marginPage.settle);
  marginPage.settle = setTimeout(() => marginSettle(s), FIND_SETTLE_MS);
  clearTimeout(marginPage.timer);
  if (marginPage.mode !== 'along') return;
  marginArm(s);
}

function marginShowReading(s, source, blocks) {
  const at = source.lastIndexOf(blocks[blocks.length - 1]);
  if (at < 0) return;
  marginPage.faceTimer = setTimeout(() => {
    if (!marginPage.reading || marginPage.s !== s) return;
    marginPage.readingAt = at;
    marginPage.readingSpan = { from: at, to: at + blocks[blocks.length - 1].length };
    const voice = $('.margin-said-aloud', marginPage.layer);
    if (voice) voice.textContent = 'Felix is reading.';
    marginRedraw();
  }, READ_FACE_AFTER_MS);
}

function marginQuiet(span) {
  const s = marginPage.s;
  const end = span && s && marginPage.layer ? s.doc.spanRects(Math.max(span.from, span.to - 200), span.to).at(-1) : null;
  if (!end) return;
  const columns = marginPage.layer.parentElement;
  const origin = columns.getBoundingClientRect();
  const quiet = document.createElement('span');
  quiet.className = 'margin-quiet';
  quiet.setAttribute('aria-hidden', 'true');
  quiet.textContent = 'Nothing to add.';
  quiet.style.height = `${Math.round(end.bottom - end.top)}px`;
  marginPage.layer.appendChild(quiet);
  const room = end.right - origin.left + 10 + quiet.offsetWidth <= columns.clientWidth;
  const x = room ? end.right - origin.left + 10 : end.right - origin.left - quiet.offsetWidth;
  const y = room ? end.top - origin.top : end.bottom - origin.top;
  quiet.style.transform = `translate(${Math.round(x)}px, ${Math.round(y)}px)`;
  setTimeout(() => {
    quiet.classList.add('leaving');
    setTimeout(() => quiet.remove(), stillMotion() ? 0 : QUIET_FADE_MS);
  }, QUIET_MS);
}

function marginReadingDone(fade = false) {
  const el = marginPage.readingEl;
  marginPage.readingAt = null;
  marginPage.readingSpan = null;
  marginPage.readingEl = null;
  marginPage.marks.get('reading')?.remove();
  marginPage.marks.delete('reading');
  marginPencilGone(fade);
  if (el && fade && !stillMotion()) {
    el.classList.add('leaving');
    setTimeout(() => el.remove(), READING_FADE_MS);
  } else el?.remove();
  marginRedraw();
}

function marginPencilGone(fade) {
  for (const [key, path] of marginThreadPaths('reading')) {
    marginPage.paths.delete(key);
    if (fade && !stillMotion()) {
      path.classList.add('leaving');
      setTimeout(() => path.remove(), READING_FADE_MS);
    } else path.remove();
  }
}

function marginLines(rects, caret) {
  const lines = [];
  for (const r of [...rects].sort((a, b) => a.top - b.top)) {
    const last = lines[lines.length - 1];
    if (last && r.top < last.bottom - 2) Object.assign(last, { left: Math.min(last.left, r.left), right: Math.max(last.right, r.right), bottom: Math.max(last.bottom, r.bottom) });
    else lines.push({ ...r });
  }
  const left = Math.min(...lines.map((l) => l.left));
  const right = Math.max(...lines.map((l) => l.right));
  const out = [];
  for (const line of lines) {
    const tall = line.bottom - line.top;
    const prev = out[out.length - 1];
    const even = caret && prev && Math.abs(prev.bottom - prev.top - tall) < 1;
    for (let top = even ? prev.bottom : Infinity; tall > 0 && top + tall <= line.top + 2; top += tall) out.push({ left, right, top, bottom: top + tall });
    out.push(line);
  }
  return out;
}

function marginSeed(id) {
  return [...id].reduce((sum, ch) => sum + ch.charCodeAt(0), 0);
}

function marginWobble(x, y, length, down = false, seed = 0, amp = 0.7) {
  const steps = Math.max(1, Math.round(length / 26));
  const w = (length / steps).toFixed(1);
  const half = (length / steps / 2).toFixed(1);
  let d = `M${x.toFixed(1)} ${y.toFixed(1)}`;
  for (let i = 0; i < steps; i++) {
    const bend = Math.round(((i + seed) % 2 ? 1 : -1) * amp * (seed ? 0.7 + ((i * 5 + seed) % 4) * 0.2 : 1) * 100) / 100;
    d += down ? ` q${bend} ${half} 0 ${w}` : ` q${half} ${bend} ${w} 0`;
  }
  return d;
}

function marginPencil(origin) {
  const span = marginPage.readingSpan;
  const block = span && marginPage.s.doc.blockAt(span.from);
  const rects = block ? marginPage.s.doc.spanRects(span.from, span.to).map((r) => ({ left: r.left - origin.left, right: r.right - origin.left, top: r.top - origin.top, bottom: r.bottom - origin.top })) : [];
  if (!rects.length) return marginPencilGone(false);
  const lines = marginLines(rects, marginPage.s.doc.isEditing() && rects.length === 2);
  const top = lines[0].top;
  const bottom = lines[lines.length - 1].bottom;
  const strokes = lines.length >= 4
    ? [{ d: marginWobble(block.getBoundingClientRect().left - origin.left - 4, top, bottom - top, true), size: bottom - top }]
    : lines.map((l) => ({ d: marginWobble(l.left, l.bottom + 4, Math.max(1, l.right - l.left)), size: Math.max(1, l.right - l.left) }));
  const total = strokes.reduce((sum, stroke) => sum + stroke.size, 0);
  const drawMs = lines.length >= 4 ? PENCIL_SIDE_MS : Math.max(PENCIL_LEAST_MS, Math.round(total / PENCIL_PX_PER_MS));
  let before = 0;
  strokes.forEach((stroke, line) => {
    const fresh = !marginPage.paths.has(`reading:pencil:${line}`);
    const path = marginPath('reading', 'pencil', line);
    path.setAttribute('d', stroke.d);
    if (fresh) {
      path.setAttribute('pathLength', '1');
      if (!stillMotion()) {
        path.style.setProperty('--pencil-d', `${Math.round((drawMs * before) / total)}ms`);
        path.style.setProperty('--pencil-t', `${Math.max(1, Math.round((drawMs * stroke.size) / total))}ms`);
        path.classList.add('sketching');
      }
    }
    before += stroke.size;
  });
  for (const [key, path] of marginThreadPaths('reading')) {
    if (Number(path.dataset.line) >= strokes.length) {
      path.remove();
      marginPage.paths.delete(key);
    }
  }
}

function marginReadingBox(origin) {
  const block = marginPage.readingAt === null ? null : marginPage.s.doc.blockAt(marginPage.readingAt);
  if (!block) return null;
  let el = marginPage.readingEl;
  if (!el) {
    el = document.createElement('div');
    el.className = 'margin-waiting';
    el.dataset.id = 'reading';
    el.setAttribute('aria-hidden', 'true');
    el.innerHTML = '<svg class="margin-drop"><use href="#leo-mark"/></svg>';
    marginPage.layer.appendChild(el);
    marginPage.readingEl = el;
  }
  const r = block.getBoundingClientRect();
  const top = r.top - origin.top;
  return { t: { id: 'reading', quote: '', messages: [] }, rects: [{ left: r.left - origin.left, right: r.left - origin.left, top, bottom: top + 20 }], el };
}

function marginTyped(id) {
  const el = marginPage.els.get(id);
  return id === marginPage.open || (!!el && !!$('textarea', el)?.value.trim());
}

async function marginRead(s) {
  if (marginPage.reading || marginPage.through) {
    marginPage.again = true;
    return;
  }
  if (s !== marginPage.s || marginPage.mode !== 'along' || !s.note.id || document.hidden || window.__LEO_VISIBLE__ === false || chat.isBusy()) return;
  if (!marginKit.changedBlocks(marginPage.baseline, s.doc.source()).length) return;
  const reading = {};
  marginPage.reading = reading;
  queueMicrotask(sideRedrawSoon);
  try {
    await flush(s);
    if (s !== marginPage.s || marginPage.mode !== 'along' || saving.unsaved()) return;
    const source = s.doc.source();
    const blocks = marginKit.changedBlocks(marginPage.baseline, source);
    if (!blocks.length) return;
    marginPage.nextRead = Date.now() + (marginPrefs?.gap ?? READ_GAP_SECONDS) * 1000;
    marginPage.newest = null;
    for (const path of marginPage.svg?.querySelectorAll('path.pencil.leaving') || []) path.remove();
    marginShowReading(s, source, blocks);
    const got = await api(`/api/notes/${enc(s.note.id)}/margin/read`, { method: 'POST', body: { version: s.note.version, blocks, keep: [...new Set([marginPage.open, ...marginPage.els.keys()])].filter((id) => id && marginTyped(id)), changed: marginChanged(s) } });
    if (s !== marginPage.s) return;
    const along = marginPage.mode === 'along';
    if (!got?.failed) marginPage.baseline = marginKit.blocksOf(source);
    if (!along) marginSetStatus('');
    else if (got?.failed) marginSetStatus(got.error || READ_FAILED);
    else marginSetStatus(got?.note ? '' : 'Felix read it; nothing to add.', true);
    if (!got?.failed && !got?.note) marginQuiet(marginPage.readingSpan);
    if (!got?.failed) marginReadingDone(!got?.note);
    if (got?.note || got?.resolved?.length) {
      const held = [];
      for (const id of got.resolved || []) {
        if (!marginTyped(id)) continue;
        const back = await api(`/api/notes/${enc(s.note.id)}/margin/threads/${enc(id)}`, { method: 'POST', body: { status: 'open', gone: marginPage.gone } }).catch((e) => {
          if (e instanceof Locked) throw e;
          return null;
        });
        if (marginPage.s !== s) return;
        if (back) held.push(id);
      }
      const now = s.doc.source();
      const fresh = got.note && marginKit.find(now, got.note);
      const crowded = fresh && marginPage.threads.some((t) => {
        const at = held.includes(t.id) && marginKit.find(now, t);
        return at && at.from < fresh.to && fresh.from < at.to;
      });
      if (crowded) {
        const kept = await api(`/api/notes/${enc(s.note.id)}/margin/threads/${enc(got.note.id)}`, { method: 'DELETE' }).then(() => false, (e) => {
          if (e instanceof Locked) throw e;
          return true;
        });
        if (marginPage.s !== s) return;
        if (!kept) got.note = null;
      }
      const freed = marginPage.threads.filter((t) => got.resolved?.includes(t.id) && !held.includes(t.id));
      for (const t of freed) marginDropThread(t.id);
      marginPage.threads = marginPage.threads.filter((t) => !freed.includes(t));
      marginPage.resolved.unshift(...freed.map(marginLight));
      if (got.note) {
        marginPage.threads.push(got.note);
        marginPage.arriving.add(got.note.id);
        marginPage.newest = got.note.id;
        const voice = $('.margin-said-aloud', marginPage.layer);
        if (voice) voice.textContent = `Felix: ${marginNow(got.note)}`;
      }
      marginRedraw();
    }
  } catch (e) {
    if (e instanceof Locked) throw e;
    if (marginPage.s === s) {
      const paused = { daily: 'Paused for today: daily limit reached', soon: 'Felix reads again in a moment', busy: 'Already reading', felix: 'Paused while Felix answers', unedited: 'Reads changes after you edit', mode: '' };
      const stale = e.status === 409 && !e.data?.why;
      if (stale) marginPage.nextRead = 0;
      if (marginPage.mode !== 'along') marginSetStatus('');
      else marginSetStatus(paused[e.data?.why] || (stale ? 'This note changed while Felix was reading. He will try again after your next edit.' : noAi(e.message) ? e.message : READ_FAILED));
    }
  } finally {
    if (s === marginPage.s && marginPage.reading === reading) {
      marginPage.reading = false;
      clearTimeout(marginPage.faceTimer);
      if (marginPage.readingAt !== null || marginPage.readingEl) marginReadingDone();
      const again = marginPage.again;
      marginPage.again = false;
      if (again && marginPage.mode === 'along') marginArm(s);
      queueMicrotask(sideRedrawSoon);
    }
  }
}

const THROUGH_MOST = 12;
const THROUGH_STOP_GRACE_MS = 400;
const THROUGH_STAGGER_MS = 80;
const THROUGH_STAGGER_MAX_MS = 640;
const READ_FAILED = "Felix could not read your note just now. Try again in a moment.";
const THROUGH_SAYS = {
  full: 'This note has many open margin notes. Resolve some, then read through again.',
  felix: 'Felix is answering in chat. Try again when he’s done.',
  busy: 'Felix is already reading. Try again in a moment.',
  changed: 'This note changed while Felix was reading. Read through again.',
  persona: 'That reader is gone. Pick another.',
};

async function marginThroughClick(persona) {
  const s = marginPage.s;
  if (!s?.note.id) return;
  const run = marginPage.through;
  if (run) {
    if (Date.now() - run.started >= THROUGH_STOP_GRACE_MS) {
      run.stop = true;
      run.abort.abort();
    }
    return;
  }
  if (chat.isBusy()) return marginSetStatus(THROUGH_SAYS.felix);
  const { parts, spread } = marginKit.passes(s.doc.source(), marginPage.threads.map((t) => t.quote));
  if (!parts.length) return marginSetStatus('Write a few lines first, then read through.');
  await marginThrough(s, parts, spread, persona);
}

async function marginThroughPart(s, run, part, label) {
  let stale = 0;
  let busy = 0;
  marginSetStatus(label);
  for (;;) {
    if (run.stop) return { notes: [] };
    try {
      await flush(s);
      if (marginPage.s !== s) return { gone: true };
      const source = s.doc.source();
      const blocks = part.blocks.filter((block) => source.includes(block));
      if (!blocks.length) return { notes: [] };
      s.doc.glow(part.from, part.to);
      let got;
      try {
        got = await api(`/api/notes/${enc(s.note.id)}/margin/analyze`, { method: 'POST', signal: run.abort.signal, body: { version: s.note.version, blocks, most: Math.min(3, THROUGH_MOST - run.added.length), gone: marginPage.gone, changed: marginChanged(s), ...(run.persona && { persona: run.persona }) } });
      } finally {
        s.doc.glow();
      }
      return got?.failed ? { say: got.error || READ_FAILED } : { notes: got?.notes || [] };
    } catch (e) {
      if (e instanceof Locked) throw e;
      if (marginPage.s !== s) return { gone: true };
      if (run.stop) return { notes: [] };
      const why = e.data?.why;
      if (why === 'persona') {
        marginPrefs = null;
        await marginLoadPrefs();
      }
      if (e.status === 409 && !why && !stale++) continue;
      if (why === 'busy' && busy++ < 3) {
        await new Promise((resolve) => setTimeout(resolve, READ_PAUSE.regular));
        continue;
      }
      return { say: noAi(e.message) ? e.message : THROUGH_SAYS[why] || (e.status === 409 && !why ? THROUGH_SAYS.changed : READ_FAILED) };
    }
  }
}

function marginReveal(s, found) {
  const source = s.doc.source();
  const order = (t) => marginKit.find(source, t)?.from ?? Number.MAX_SAFE_INTEGER;
  const sorted = [...found].sort((a, b) => order(a) - order(b));
  if (stillMotion()) {
    marginPage.threads.push(...sorted);
    marginRedraw();
    return;
  }
  const step = sorted.length > 1 ? Math.min(THROUGH_STAGGER_MS, THROUGH_STAGGER_MAX_MS / (sorted.length - 1)) : 0;
  sorted.forEach((t, i) => {
    const show = () => {
      if (marginPage.s !== s || t.gone) return;
      marginPage.arriving.add(t.id);
      marginPage.threads.push(t);
      marginRedraw();
    };
    if (i) setTimeout(show, i * step);
    else show();
  });
}

function marginNotes(n) {
  return `${n} margin note${n === 1 ? '' : 's'}`;
}

function marginThroughUndo(s, added) {
  return async () => {
    let gone = 0;
    let kept = 0;
    let failed = false;
    for (const t of added) {
      try {
        await api(`/api/notes/${enc(s.note.id)}/margin/threads/${enc(t.id)}`, { method: 'DELETE' });
      } catch (e) {
        if (e instanceof Locked) throw e;
        if (e.status === 409 && ['replying', 'replied'].includes(e.data?.why)) kept++;
        else if (e.status !== 404) failed = true;
        if (e.status !== 404) continue;
      }
      gone++;
      t.gone = true;
      if (marginPage.s === s) {
        marginPage.threads = marginPage.threads.filter((old) => old.id !== t.id);
        marginDropThread(t.id);
      }
    }
    if (marginPage.s === s) marginDraw();
    if (failed) toast('Could not take them out. Try again.', { bad: true });
    else toast(`Took out ${marginNotes(gone)}.${kept ? ` You replied to ${kept}, so ${kept === 1 ? 'it stays' : 'they stay'}.` : ''}`);
  };
}

async function marginThrough(s, parts, spread, persona) {
  const run = { started: Date.now(), stop: false, added: [], abort: new AbortController(), persona };
  marginPage.through = run;
  let say = '';
  try {
    for (let i = 0; i < parts.length && !run.stop && run.added.length < THROUGH_MOST; i++) {
      const got = await marginThroughPart(s, run, parts[i], parts.length > 1 ? `Reading through… ${i + 1} of ${parts.length}` : 'Reading through…');
      if (got.gone) return;
      run.added.push(...(got.notes || []));
      if (got.say) {
        say = got.say;
        break;
      }
    }
  } finally {
    if (marginPage.through === run) marginPage.through = null;
    if (marginPage.s === s) {
      s.doc.glow();
        }
  }
  if (marginPage.s !== s) return;
  const count = run.added.length;
  if (count) marginReveal(s, run.added);
  if (say) marginSetStatus(say);
  else if (!count && !run.stop) marginSetStatus('Nothing to add this time', true);
  else marginSetStatus(spread ? 'This note is long, so Felix read parts from all through it.' : '');
  if (count) toast(`${run.stop ? 'Stopped. ' : ''}Felix added ${marginNotes(count)}`, { action: 'Undo', run: marginThroughUndo(s, run.added) });
  if (marginPage.mode === 'along') marginArm(s);
}

const MARGIN_DIAGRAMS = {
  both: '<svg viewBox="0 0 120 52" aria-hidden="true"><path class="d-page" d="M42 8h36v36H42z"/><path class="d-felix" d="M6 12h28M6 17h22M86 28h28M86 33h20"/></svg>',
  right: '<svg viewBox="0 0 120 52" aria-hidden="true"><path class="d-page" d="M14 8h60v36H14z"/><path class="d-felix" d="M82 14h28M82 19h22M82 32h26M82 37h18"/></svg>',
  marks: '<svg viewBox="0 0 120 52" aria-hidden="true"><path class="d-page" d="M26 8h68v36H26z"/><path class="d-text" d="M33 18h40M33 26h48M33 34h30"/><rect class="d-mark" x="76" y="16" width="5" height="5"/><rect class="d-mark" x="66" y="32" width="5" height="5"/></svg>',
};

const PACE_DIAGRAMS = Object.fromEntries(Object.entries({ fast: 10, regular: 24, slow: 48 }).map(([pace, wait]) => [pace, `<svg viewBox="0 0 120 52" aria-hidden="true"><path class="d-page" d="M12 8h96v36H12z"/><path class="d-text" d="M20 26h24"/><path class="d-felix" d="M48 26h${wait}"/><rect class="d-mark" x="${51 + wait}" y="23" width="6" height="6"/></svg>`]));

function marginWait(seconds) {
  return seconds < 120 ? `${seconds} sec` : `${seconds / 60} min`;
}

function felixRhythm(view) {
  const pause = (READ_PAUSE[view.pace] || READ_PAUSE.regular) / 1000;
  return `After ${pause} sec of quiet (longer mid-sentence), at most once every ${marginWait(view.gap)}.`;
}

function felixReads(view) {
  const label = view.cap ? `${view.today} of ${view.cap} reads today` : `${view.today} reads today, no daily limit`;
  return `<span class="badge" id="felix-reads" title="${label}" aria-label="${label}">${view.today}/${view.cap || '∞'} today</span>`;
}

function felixPersonaForm(view, p) {
  const icons = (view.persona_icons || []).map((icon) => `<label class="persona-tile" title="${esc(PERSONA_ICON_NAMES[icon] || icon)}"><input type="radio" name="persona-icon" value="${esc(icon)}" aria-label="${esc(PERSONA_ICON_NAMES[icon] || icon)}"${icon === p.icon ? ' checked' : ''}><span class="persona-icon" aria-hidden="true">${PERSONA_ICON[icon] || ''}</span></label>`).join('');
  const long = Array.from(p.details.trim()).length > MARGIN_INSTRUCTIONS_READS;
  return `<form class="persona-form" data-read-field data-persona-form="${esc(p.id)}"><div class="persona-tiles" role="radiogroup" aria-label="Icon">${icons}</div><input class="set-input" name="name" required placeholder="Name" aria-label="Name" value="${esc(p.name)}"><textarea class="set-input" name="says" data-reads="${MARGIN_INSTRUCTIONS_READS}" rows="3" required placeholder="What should he look for? For example: check each claim against its evidence." aria-label="What should he look for?">${esc(p.details)}</textarea><p class="hint" data-read-hint role="status"${long ? '' : ' hidden'}>Felix reads the first ${MARGIN_INSTRUCTIONS_READS} characters of this.</p><div class="persona-form-acts"><button type="submit" class="btn primary sm">Save</button><button type="button" class="btn plain sm" data-persona-act="cancel">Cancel</button>${p.id ? '<button type="button" class="btn plain sm danger-text" data-persona-act="delete">Delete</button>' : ''}</div></form>`;
}

function felixPersonaCard(view) {
  const list = view.personas || [];
  const now = list.find((p) => p.id === view.default_persona) || list[0];
  if (!now) return '';
  if (personaStartNew !== null) {
    personaReturn = personaStartNew || null;
    personaStartNew = null;
    personaEditing = 'new';
  }
  const editing = personaEditing === 'new' ? { id: '', name: '', icon: (view.persona_icons || ['flask'])[0], details: '' } : list.find((p) => p.custom && p.id === personaEditing);
  if (editing) return `<section class="set-card" data-find="felix-persona" id="felix-persona-card"><header><h3>${editing.id ? 'Edit persona' : 'New persona'}</h3></header>${felixPersonaForm(view, editing)}</section>`;
  personaEditing = null;
  const tiles = list.map((p) => `<div class="persona-slot"><label class="persona-tile" title="${esc(p.name)}"><input type="radio" name="felix-persona" value="${esc(p.id)}" data-margin-pref="default_persona"${p.id === now.id ? ' checked' : ''}><span class="persona-icon" aria-hidden="true">${PERSONA_ICON[p.icon] || PERSONA_ICON.book}</span><span>${esc(p.name)}</span></label>${p.custom ? `<button type="button" class="persona-edit" data-persona-act="edit" data-id="${esc(p.id)}" aria-label="Edit ${esc(p.name)}" title="Edit">${PERSONA_ICON.pen}</button>` : ''}</div>`).join('');
  const add = list.filter((p) => p.custom).length < (view.most_personas || 8) ? `<button type="button" class="persona-tile add" data-persona-act="new"><span class="persona-icon" aria-hidden="true">${PERSONA_ICON.plus}</span><span>New</span></button>` : '';
  return `<section class="set-card" data-find="felix-persona" id="felix-persona-card"><header><h3>Who reads your notes</h3></header><p class="hint">New notes start with this one. Each note can switch. Felix chat stays the same.</p><div class="persona-tiles" role="radiogroup" aria-label="Who reads your notes">${tiles}${add}</div>
    <div class="persona-line"><span>${esc(now.line || 'Your own persona.')}</span><button type="button" class="link-btn" data-persona-act="details" aria-expanded="${personaDetails}">${personaDetails ? 'Hide details' : 'See details'}</button></div>${personaDetails ? `<p class="persona-details">${esc(now.details)}</p>` : ''}</section>`;
}

function felixPersonaRedraw(view, focus = '') {
  const card = $('#felix-persona-card');
  if (!card) return;
  const from = card.getBoundingClientRect().height;
  const wasForm = !!$('form', card);
  card.outerHTML = felixPersonaCard(view);
  const drawn = $('#felix-persona-card');
  if (focus) $(focus, drawn)?.focus({ preventScroll: true });
  slideHeight(drawn, from, drawn.getBoundingClientRect().height);
  if (!!$('form', drawn) === wasForm || stillMotion()) return;
  for (const part of drawn.children) part.animate([{ opacity: 0, transform: 'translateY(4px)' }, { opacity: 1, transform: 'none' }], { duration: MOTION_MS, easing: EASE_OUT });
}

async function felixSavePersonas(list) {
  const view = await api('/api/margin', { method: 'POST', body: { personas: list } });
  marginRemember(view);
  state.margin = view;
  return view;
}

async function felixFreshCustoms() {
  const view = await api('/api/margin');
  marginRemember(view);
  state.margin = view;
  return felixCustoms(view);
}

function felixCustoms(view) {
  return (view.personas || []).filter((p) => p.custom).map((p) => ({ id: p.id, name: p.name, icon: p.icon, says: p.details }));
}

document.addEventListener('click', async (e) => {
  const button = e.target.closest?.('[data-persona-act]');
  const view = state.margin;
  if (!button || !view) return;
  const act = button.dataset.personaAct;
  if (act === 'delete' && !button.dataset.armed) {
    button.dataset.armed = '1';
    button.textContent = 'Delete for good';
    return;
  }
  try {
    if (act === 'delete') {
      await felixSavePersonas((await felixFreshCustoms()).filter((p) => p.id !== personaEditing));
      personaEditing = null;
    } else if (act === 'new') personaEditing = 'new';
    else if (act === 'edit') personaEditing = button.dataset.id;
    else if (act === 'cancel') {
      personaEditing = null;
      personaReturn = null;
    }
    else if (act === 'details') {
      const shown = personaDetails && $('#felix-persona-card .persona-details');
      personaDetails = !personaDetails;
      if (shown) {
        const card = shown.parentElement;
        const from = card.getBoundingClientRect().height;
        shown.hidden = true;
        const to = card.getBoundingClientRect().height;
        shown.hidden = false;
        return slideHeight(card, from, to, () => (shown.remove(), felixPersonaRedraw(state.margin, '[data-persona-act="details"]')));
      }
    }
  } catch (error) {
    return fail(error);
  }
  felixPersonaRedraw(state.margin, act === 'new' || act === 'edit' ? '[name="name"]' : act === 'details' ? '[data-persona-act="details"]' : '[name="felix-persona"]:checked');
});

document.addEventListener('submit', async (e) => {
  const form = e.target.closest?.('[data-persona-form]');
  if (!form || !state.margin) return;
  e.preventDefault();
  const id = form.dataset.personaForm;
  const entry = { id: id || null, name: $('[name="name"]', form).value, icon: $('[name="persona-icon"]:checked', form)?.value || 'flask', says: $('[name="says"]', form).value };
  const save = $('[type="submit"]', form);
  save.disabled = true;
  try {
    const customs = await felixFreshCustoms();
    const view = await felixSavePersonas(id ? customs.map((p) => (p.id === id ? entry : p)) : [...customs, entry]);
    personaEditing = null;
    const made = id ? null : felixCustoms(view).find((p) => !customs.some((old) => old.id === p.id));
    if (personaReturn) return felixPersonaBack(made?.id);
    felixPersonaRedraw(state.margin, '[name="felix-persona"]:checked');
  } catch (error) {
    save.disabled = false;
    toast('Could not save this persona. Check the name and what he should look for.');
  }
});

async function felixPersonaBack(persona) {
  const note = personaReturn;
  personaReturn = null;
  if (persona) await api(`/api/notes/${enc(note)}/margin`, { method: 'POST', body: { persona } }).catch(fail);
  location.hash = `#/n/${note}`;
}

function felixSettingsCards(view, page) {
  if (!view) personaStartNew = null;
  if (!view) return '<section class="set-card"><p class="hint">Felix’s settings are not available right now.</p></section>';
  const provider = page.tasks.find((task) => task.task === 'writing')?.provider || '';
  const name = page.tasks.find((task) => task.task === 'writing')?.choices?.find((choice) => choice.id === provider)?.label || 'your writing AI';
  const where = `Reads along sends the note’s title and the text you change to ${esc(name)}${page.reads_stay_here ? ', which runs on this computer' : ''}. Read through sends the whole note, a part at a time.`;
  return `<section class="set-card" data-find="felix-new-notes"><header><h3>New notes start</h3></header><p class="hint">${where}</p>${choiceCards({ label: 'New notes start', field: 'data-margin-pref="default_mode"', current: view.default_mode, choices: [['along', 'Reads along', 'Comments after a pause, only when it matters.'], ['ask', 'When you ask', `Select text, then Ask Felix or ${marginAskKeys()}.`]] })}</section>
    ${felixPersonaCard(view)}
    <section class="set-card" data-find="felix-where"><header><h3>Where notes appear</h3></header>${choiceCards({ label: 'Where notes appear', field: 'data-margin-pref="layout"', current: view.layout, choices: [['both', 'Both margins', 'Your words in the middle.', MARGIN_DIAGRAMS.both], ['right', 'Right margin', 'One column beside you.', MARGIN_DIAGRAMS.right], ['marks', 'Marks only', 'A small mark to open.', MARGIN_DIAGRAMS.marks]] })}</section>
    <section class="set-card" data-find="felix-auto-model"><header><h3>Which model answers</h3></header><div class="set-row"><span class="set-label">Cheaper model for lookups</span>${settingsSwitch('data-margin-pref="auto_model"', view.auto_model !== false, 'Cheaper model for lookups')}</div><p class="hint">Short lookups use the cheapest model of your writing AI. Everything else uses the model you chose, and a chat never moves down while it is open.</p></section>
    <section class="set-card felix-rhythm"><header><h3>How often he reads</h3>${felixReads(view)}</header><p class="hint" id="felix-rhythm" aria-live="polite">${esc(felixRhythm(view))}</p><div class="set-row column" data-find="felix-pace"><span class="set-label">Reading pace</span>${choiceCards({ label: 'Reading pace', field: 'data-margin-pref="pace"', current: view.pace, choices: [['fast', 'Fast', `After ${READ_PAUSE.fast / 1000} sec of quiet, longer mid-sentence.`, PACE_DIAGRAMS.fast], ['regular', 'Regular', `After ${READ_PAUSE.regular / 1000} sec of quiet, longer mid-sentence.`, PACE_DIAGRAMS.regular], ['slow', 'Slow', `After ${READ_PAUSE.slow / 1000} sec of quiet, longer mid-sentence.`, PACE_DIAGRAMS.slow]] })}</div>
    <div class="set-row" data-find="felix-gap"><span class="set-label">Time between reads</span>${segmented({ label: 'Time between reads', field: 'data-margin-pref="gap"', current: String(view.gap), choices: (view.gaps || []).map((gap) => [String(gap), marginWait(gap)]) })}</div>
    <div class="set-row" data-find="felix-cap"><span class="set-label">Reads a day</span>${segmented({ label: 'Reads a day', field: 'data-margin-pref="cap"', current: String(view.cap), choices: (view.caps || []).map((cap) => [String(cap), cap ? String(cap) : 'No limit']) })}</div>
    ${view.problem ? `<p class="set-note warn" role="status">${esc(view.problem)}</p>` : ''}</section>`;
}

document.addEventListener('change', async (e) => {
  const field = e.target.closest?.('[data-margin-pref]');
  if (!field) return;
  const key = field.dataset.marginPref;
  const value = key === 'auto_model' ? field.checked : key === 'cap' || key === 'gap' ? Number(field.value) : field.value;
  try {
    const view = await api('/api/margin', { method: 'POST', body: { [key]: value } });
    marginRemember(view);
    if (state.view === 'settings') state.margin = view;
    if (key === 'default_persona') felixPersonaRedraw(view, '[name="felix-persona"]:checked');
    const rhythm = $('#felix-rhythm');
    if (rhythm) rhythm.textContent = felixRhythm(view);
    const reads = $('#felix-reads');
    if (reads) reads.outerHTML = felixReads(view);
  } catch (error) {
    if (key === 'auto_model') field.checked = !value;
    if (key === 'default_persona' && state.margin) felixPersonaRedraw(state.margin, '[name="felix-persona"]:checked');
    fail(error);
  }
});


document.addEventListener('pointerdown', (e) => {
  if (!e.target.closest?.('#margin-mode, #margin-mode-menu')) marginModeMenu(false);
  if (!e.target.closest?.('#margin-persona, #margin-persona-menu')) marginPersonaMenu(false);
});

function marginShown() {
  const s = marginPage.s;
  if (!s || marginPage.mode !== 'along') return;
  marginArm(s);
}
document.addEventListener('visibilitychange', marginShown);
window.addEventListener?.('leo-visibility', marginShown);
