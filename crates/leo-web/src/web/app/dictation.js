const DICTATION_COMMON_WORDS = new Set(
  `the and for not with you this but his from they say her she will one all would
  there their what out about who get which when make can like time just him know
  take into year your good some could them see other than then now look only come
  over think also back after use two how our work first well way even new want
  because any these give day most are was were been has had did does said went
  made got came took saw knew thought where why here very much many still too again
  off down never every own same another both each few more less last next while
  before through under between should might must being have that its yes okay`.split(/\s+/)
);
let dictationShortcutNote = null, dictationCaptureArmed = false;
const dictationOriginals = new Set();
function dictationKeycaps(keys) {
  return shortcutParts(keys).map((part) => `<kbd>${esc(part)}</kbd>`).join('');
}
function dictationShortcutField(shortcut, saved) {
  return shortcut.recording ? `<span class="shortcut-prompt">Press your keys\u2026</span><span class="shortcut-held">${dictationKeycaps(shortcut.held)}</span>` : dictationKeycaps(saved);
}
function dictationSection(name) {
  return name === 'dictation' || name === 'shortcuts';
}
function dictationShortcutRow(view) {
  const s = view.settings || {}, keys = view.shortcut || {};
  return `<div class="set-row column" data-find="shortcut-dictation"><div class="set-line"><span class="set-label">Dictation (works in any app)</span><span class="set-shortcut dictation-keys"><input type="hidden" data-dictation="shortcut" value="${esc(s.shortcut || '')}"><span class="shortcut-field${keys.recording ? ' recording' : ''}" id="dictation-shortcut-field" role="group" aria-label="Dictation shortcut">${dictationShortcutField(keys, s.shortcut)}</span><button class="btn plain sm" data-action="dictation-record">Change</button></span></div><span id="dictation-shortcut-status" class="hint${keys.problem && !keys.recording ? ' bad' : ''}" aria-live="polite">${esc(keys.recording ? 'Esc to cancel' : keys.problem || '')}</span></div>`;
}
function dictationCards(view) {
  if (!view) return '';
  const s = view.settings || {};
  const select = (name, choices) => `<span class="select-control"><select data-dictation="${name}">${choices.map(([value, label]) => settingsOption(value, label, String(s[name] == null ? '' : s[name]))).join('')}</select>${ICON.chevron}</span>`;
  const row = (label, control, find) => `<label class="set-row" data-find="${find}"><span class="set-label">${label}</span><span class="set-value">${control}</span></label>`;
  const toggle = (name, label, find, about) => `<label class="set-row" data-find="${find}"><span class="grow">${label}<span class="set-desc">${about}</span></span>${settingsSwitch(`data-dictation="${name}"`, s[name])}</label>`;
  const choice = (name, label, choices, find) => `<div class="set-row" data-find="${find}"><span class="set-label">${label}</span>${segmented({ label, choices, current: String(s[name] == null ? choices[0][0] : s[name]), field: `data-dictation="${name}"` })}</div>`;
  const microphones = [['', 'System default']].concat((view.microphones || []).map((name) => [name, name]));
  return `<section class="set-card" id="dictation-preferences"><header><h3>Activation and microphone</h3><span class="set-value"><span class="hint" id="dictation-result" role="status"></span></span></header>
    ${view.problem ? `<p class="set-note warn">${esc(view.problem)}</p>` : ''}
    <p class="hint">Custom words and filler rules run on this computer, without writing AI unless you turn on AI writing below.</p>
    ${choice('mode', 'Activation', [['both', 'Hold or tap'], ['hold', 'Hold'], ['tap', 'Tap']], 'dictation-activation')}
    ${row('Microphone', select('microphone', microphones), 'dictation-microphone')}
    ${toggle('sounds', 'Start and stop sounds', 'dictation-sounds', 'Play a short sound when dictation starts and stops.')}
  </section><section class="set-card"><header><h3>Text</h3></header>
    <label class="set-row column" data-find="dictation-words"><span class="set-label">Custom words</span><textarea class="set-input" data-dictation="custom_words" rows="3" placeholder="Names and terms, one per line">${esc((s.custom_words || []).join('\n'))}</textarea><span class="hint">Names and terms leo should spell your way, up to 200. Some models also listen for them; the first ones count most.</span></label>
    ${toggle('fillers', 'Remove filler words', 'dictation-fillers', 'Take out words like um and uh as you speak.')}
    <div class="set-row column" data-find="dictation-ai"><label class="set-line"><span class="grow">AI writing</span>${settingsSwitch(`data-dictation="writing" data-on="${s.writing && s.writing !== 'off' ? esc(s.writing) : 'quick'}"`, s.writing && s.writing !== 'off')}</label><span class="hint">Fixes grammar, writes times like 2:30 and formats lists with your <a href="#/settings/writing">Writing AI</a>. Matches the tone of the app you dictate into (only the app’s name is used). Adds a few seconds; Escape cancels.</span>${view.writing_ready === false ? '<span class="set-note warn">No writing AI is set up yet, so dictations are pasted as spoken. <a href="#/settings/writing">Set up writing AI</a>.</span>' : ''}</div>
  </section><section class="set-card"><header><h3>Popup</h3></header>
    ${choice('popup', 'Popup', [['live', 'Live'], ['minimal', 'Minimal'], ['off', 'Off']], 'dictation-popup')}
    <div class="set-row" id="dictation-position-row"${s.popup === 'off' ? ' hidden' : ''}><span class="set-label">Position</span>${segmented({ label: 'Popup position', choices: [['false', 'Bottom'], ['true', 'Top']], current: String(s.top == null ? 'false' : s.top), field: 'data-dictation="top"' })}</div>
  </section><section class="set-card" data-find="dictation-recent"><header><h3>Recent dictations</h3><button class="btn plain sm" data-action="dictation-clear">Clear history</button></header><p class="hint">Last 20, on this computer only.</p><div id="dictation-recent">${dictationRecent(view.recent || [])}</div></section>`;
}
let dictationEdit = null;
const dictationOffers = {}, DICTATION_MOST_WORDS = 200;
function dictationRecent(items) {
  return items.length ? items.map((item) => {
    const id = esc(String(item.id));
    const text = dictationEdit && dictationEdit.id === item.id
      ? `<input class="set-input grow" data-edit-text value="${esc(dictationEdit.text)}" aria-label="Edit dictation">`
      : `<span class="grow" role="button" tabindex="0" title="Edit" data-action="dictation-edit" data-id="${id}">${esc(item.text)}</span>`;
    const offers = (dictationOffers[item.id] || []).map((word) => `<button class="chip accent" data-action="dictation-add-word" data-id="${id}" data-word="${esc(word)}">${ICON.plus}Add \u201c${esc(word)}\u201d to custom words</button>`).join('');
    const shown = item.original && dictationOriginals.has(item.id);
    const edited = item.edited ? `<span class="hint">Edited${item.original ? ` \u00b7 <button type="button" class="link-btn" data-action="dictation-original" data-id="${id}" aria-expanded="${shown ? 'true' : 'false'}">${shown ? 'Hide' : 'Show'} original</button>` : ''}</span>` : '';
    return `<div class="set-row" data-refresh-key="${id}">${text}${edited}<button class="btn plain sm" data-action="dictation-copy" data-id="${id}">Copy</button></div>${shown ? `<p class="set-note" data-refresh-key="original-${id}">Original: ${esc(item.original)}</p>` : ''}${offers ? `<div class="card-meta" data-refresh-key="offers-${id}">${offers}</div>` : ''}`;
  }).join('') : '<p class="set-note">Your dictations will appear here.</p>';
}
function toggleDictationOriginal(el) {
  const id = Number(el.dataset.id);
  if (!dictationOriginals.delete(id)) dictationOriginals.add(id);
  showDictationRecent();
}
function showDictationRecent() {
  refreshPanel($('#dictation-recent'), dictationRecent((state.dictation && state.dictation.recent) || []));
}
function dictationWordOffers(before, after, known) {
  const seen = new Set(before.split(/\s+/)), runs = [];
  let run = [];
  for (const word of after.split(/\s+/).filter(Boolean)) {
    if (seen.has(word)) { if (run.length) runs.push(run); run = []; } else run.push(word);
  }
  if (run.length) runs.push(run);
  const lower = new Set(known.map((word) => word.toLowerCase()));
  const ordinary = (word) => { const bare = word.toLowerCase().replace(/[^\p{L}\p{N}]/gu, ''); return [...bare].length < (/[^\p{Script=Latin}\p{N}]/u.test(bare) ? 2 : 3) || DICTATION_COMMON_WORDS.has(bare); };
  const phrases = runs.map((r) => r.join(' ').replace(/^[^\p{L}\p{N}]+|[^\p{L}\p{N}]+$/gu, '')).filter((phrase) => {
    const key = phrase.toLowerCase();
    if (!phrase || phrase.split(/\s+/).every(ordinary) || before.includes(phrase) || lower.has(key)) return false;
    lower.add(key);
    return true;
  });
  return phrases.slice(0, 3);
}
function startDictationEdit(el) {
  const id = Number(el.dataset.id), item = ((state.dictation && state.dictation.recent) || []).find((i) => i.id === id);
  if (!item) return;
  dictationEdit = { id, text: item.text };
  showDictationRecent();
  const input = $('[data-edit-text]');
  if (input) { input.focus(); input.select(); }
}
function cancelDictationEdit() {
  dictationEdit = null;
  showDictationRecent();
}
async function saveDictationEdit() {
  const edit = dictationEdit, text = edit.text.trim();
  const before = (state.dictation.recent.find((i) => i.id === edit.id) || {}).text || '';
  dictationEdit = null;
  try {
    if (text && text !== before) {
      await dictationChange({ action: 'edit', id: edit.id, text });
      dictationOffers[edit.id] = dictationWordOffers(before, text, state.dictation.settings.custom_words || []);
    }
  } finally {
    showDictationRecent();
  }
}
function addDictationWord(el) {
  const id = Number(el.dataset.id), word = el.dataset.word, words = dictationDraft().custom_words;
  if (!words.some((w) => w.toLowerCase() === word.toLowerCase())) {
    if (words.length >= DICTATION_MOST_WORDS) {
      const result = $('#dictation-result');
      if (result) result.textContent = `Custom words is full (${DICTATION_MOST_WORDS}). Remove one to add more.`;
      return;
    }
    $('[data-dictation="custom_words"]').value = words.concat(word).join('\n');
  }
  dictationOffers[id] = (dictationOffers[id] || []).filter((w) => w !== word);
  showDictationRecent();
  return saveDictation();
}
function dictationDraft() {
  const settings = Object.assign({}, state.dictation.settings);
  for (const field of app.querySelectorAll('[data-dictation]')) {
    const name = field.dataset.dictation;
    settings[name] = name === 'custom_words' ? field.value.split('\n').map((word) => word.trim()).filter(Boolean) : name === 'writing' ? (field.checked ? field.dataset.on : 'off') : field.type === 'checkbox' ? field.checked : name === 'top' ? field.value === 'true' : /microphone/.test(name) ? field.value || null : field.value;
  }
  return settings;
}
let dictationGeneration = 0;
async function dictationChange(change, mine = seq) {
  const generation = mine === seq ? ++dictationGeneration : dictationGeneration;
  const view = await api('/api/dictation', { method: 'POST', body: change });
  if (mine !== seq || generation !== dictationGeneration || state.view !== 'settings') return;
  state.dictation = view;
  updateDictationStatus(view);
  const recent = $('#dictation-recent');
  refreshPanel(recent, dictationRecent(view.recent || []));
  const result = $('#dictation-result');
  if (result && change.action === 'save') {
    result.textContent = 'Saved';
    result.classList.remove('flash');
    void result.offsetWidth;
    result.classList.add('flash');
    pulse(result);
    clearTimeout(dictationSavedTimer);
    dictationSavedTimer = setTimeout(() => { if (result.isConnected) result.textContent = ''; }, 2000);
  }
  pollDictation(mine, generation);
}
function clearDictationsAsk() {
  return confirmSheet({ title: 'Clear recent dictations?', text: 'The last 20 dictations are removed from this computer. This cannot be undone.', yes: 'Clear history', action: 'dictation-clear-now' });
}
function clearDictationsNow() {
  closeSheet();
  return dictationChange({ action: 'clear' });
}
function dictationRecording() {
  return state.view === 'settings' && !!(state.dictation && state.dictation.shortcut && state.dictation.shortcut.recording);
}
function stopDictationRecording() {
  return dictationChange({ action: 'resume_shortcut' }).catch(fail);
}
async function captureDictationShortcut(keys) {
  dictationCaptureArmed = false;
  const field = $('[data-dictation="shortcut"]'), before = field && field.value;
  if (field) field.value = keys;
  try {
    await saveDictation(Object.assign(dictationDraft(), { shortcut: keys }));
    const shown = shortcutParts(keys).join(' '), mode = state.dictation && state.dictation.settings && state.dictation.settings.mode;
    dictationShortcutNote = { text: mode === 'hold' ? `Saved. Hold ${shown} to dictate.` : mode === 'tap' ? `Saved. Press ${shown} to start and stop.` : `Saved. Hold or press ${shown} to dictate.`, ok: true };
  } catch (e) {
    if (field) field.value = before;
    await dictationChange({ action: 'resume_shortcut' }).catch(() => {});
    dictationShortcutNote = { text: e.message, ok: false };
  }
  if (state.view === 'settings' && state.dictation) updateDictationStatus(state.dictation);
}
function updateDictationStatus(view) {
  const shortcut = view.shortcut || {};
  if (shortcut.recording) { dictationShortcutNote = null; dictationCaptureArmed = true; }
  const status = $('#dictation-shortcut-status'), field = $('#dictation-shortcut-field');
  if (status) {
    const note = shortcut.recording ? null : dictationShortcutNote;
    status.textContent = shortcut.recording ? 'Esc to cancel' : note ? note.text : shortcut.problem || '';
    status.classList.toggle('ok', !!(note && note.ok));
    status.classList.toggle('bad', !shortcut.recording && !(note && note.ok) && !!(note || shortcut.problem));
  }
  if (field) {
    const html = dictationShortcutField(shortcut, (view.settings || {}).shortcut);
    if (field.innerHTML !== html) field.innerHTML = html;
    field.classList.toggle('recording', !!shortcut.recording);
  }
  if (shortcut.captured && !shortcut.recording && dictationCaptureArmed) captureDictationShortcut(shortcut.captured).catch(fail);
}
function dictationPolling(mine, generation) {
  const here = state.view === 'settings' && dictationSection(state.section) && appVisible();
  if (!here) dictationCaptureArmed = false;
  return here && mine === seq && generation === dictationGeneration && !!state.dictation;
}
async function pollDictation(mine, generation) {
  if (!dictationPolling(mine, generation)) return;
  await new Promise((resolve) => setTimeout(resolve, dictationRecording() ? 150 : 600));
  if (!dictationPolling(mine, generation)) return;
  try {
    const view = await api('/api/dictation');
    if (mine !== seq || generation !== dictationGeneration || state.view !== 'settings') return;
    state.dictation = view;
    updateDictationStatus(view);
    const recent = $('#dictation-recent');
    refreshPanel(recent, dictationRecent(view.recent || []));
  } catch (e) {}
  pollDictation(mine, generation);
}
let dictationSaving = Promise.resolve(), dictationTyping = null, dictationSavedTimer = null;
function saveDictation(settings = state.dictation && dictationDraft(), mine = seq, current = false) {
  if (!settings) return dictationSaving;
  clearTimeout(dictationTyping);
  const now = () => current && state.dictation && state.dictation.settings ? Object.assign({}, settings, { shortcut: state.dictation.settings.shortcut }) : settings;
  dictationSaving = dictationSaving.catch(() => {}).then(() => dictationChange({ action: 'save', settings: now() }, mine));
  return dictationSaving;
}
app.addEventListener('change', (e) => {
  if (e.target.closest?.('[data-dictation]') && e.target.dataset.dictation !== 'shortcut') {
    if (e.target.dataset.dictation === 'popup') $('#dictation-position-row').hidden = e.target.value === 'off';
    saveDictation().catch(fail);
  }
});
app.addEventListener('keydown', (e) => {
  const el = e.target;
  if (el.matches?.('[data-edit-text]')) {
    if (e.isComposing || (e.key !== 'Enter' && e.key !== 'Escape')) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.key === 'Escape') cancelDictationEdit();
    else saveDictationEdit().catch(fail);
  } else if ((e.key === 'Enter' || e.key === ' ') && el.dataset?.action === 'dictation-edit') {
    e.preventDefault();
    startDictationEdit(el);
  }
});
app.addEventListener('input', (e) => {
  if (e.target.matches?.('[data-edit-text]') && dictationEdit) dictationEdit.text = e.target.value;
  if (e.target.dataset?.dictation !== 'custom_words') return;
  clearTimeout(dictationTyping);
  const settings = dictationDraft(), mine = seq;
  dictationTyping = setTimeout(() => saveDictation(settings, mine, true).catch(fail), 700);
});
if (typeof window !== 'undefined' && window.addEventListener) {
  const holdKeys = (e) => {
    if (!dictationRecording()) return;
    e.preventDefault();
    e.stopImmediatePropagation();
    if (e.type === 'keydown' && e.key === 'Escape') stopDictationRecording();
  };
  const forgetCapture = () => { if (!appVisible()) dictationCaptureArmed = false; };
  window.addEventListener('leo-visibility', forgetCapture);
  if (typeof document !== 'undefined' && document.addEventListener) document.addEventListener('visibilitychange', forgetCapture);
  window.addEventListener('keydown', holdKeys, true);
  window.addEventListener('keyup', holdKeys, true);
  window.addEventListener('click', (e) => {
    if (dictationRecording() && !e.target.closest?.('.dictation-keys')) stopDictationRecording();
  }, true);
}
if (typeof window !== 'undefined' && window.leoTheme) {
  const syncDictationTheme = () => api('/api/dictation', { method: 'POST', body: { action: 'theme', theme: window.leoTheme.choice() } }).catch(() => {});
  window.leoTheme.onChange(syncDictationTheme);
  syncDictationTheme();
}
