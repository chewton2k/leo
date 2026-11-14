// Settings sections, grouped as the sidebar shows them: [id, label, icon, description].
const SETTINGS_SECTIONS = [
  { group: 'leo', items: [
    ['general', 'General', 'sliders', 'Appearance, and how leo starts with your computer.'],
    ['shortcuts', 'Shortcuts', 'command', 'Keys for the things you do most.'],
    ['dictation', 'Dictation', 'keyboard', 'Speak into any app with a shortcut. Speech stays on this computer.'],
    ['models', 'Speech models', 'wave', 'The local model that turns speech into text.'],
    ['writing', 'Writing AI', 'pen', 'The AI that writes your notes and powers Felix.'],
    ['felix', 'Felix', 'chat', 'How Felix reads along beside your notes.'],
  ] },
  { group: 'Your notes', items: [
    ['calendar', 'Calendar', 'calendar', 'Name recordings after the class or meeting you are in.'],
  ] },
  { group: 'Data', items: [
    ['keeping', 'Keeping', 'storage', 'How long leo keeps each kind of data on this computer.'],
    ['trash', 'Trash', 'trash', 'Deleted notes wait here, so a mistake can be undone.'],
  ] },
  { group: 'This app', items: [
    ['health', 'Health', 'pulse', 'Check that recording, dictation and AI are ready.'],
    ['updates', 'Updates & about', 'update', 'Keep leo up to date.'],
  ] },
];
const SETTINGS_INDEX = [
  { section: 'general', id: 'theme', label: 'Theme', keywords: 'appearance dark light match device mode colours colors night' },
  { section: 'general', id: 'felix-look', label: 'Felix look', keywords: 'appearance cute demon character mascot horns' },
  { section: 'general', id: 'note-width', label: 'Note width', keywords: 'appearance comfortable wide full width reading column margin page' },
  { section: 'general', id: 'text-size', label: 'Text size', keywords: 'appearance small medium large font reading bigger smaller zoom' },
  { section: 'general', id: 'felix-lettering', label: "Felix's lettering", keywords: 'appearance font typeface serif letters notes writing felix own' },
  { section: 'general', id: 'open-at-login', label: 'Open at login', keywords: 'start startup boot launch computer' },
  { section: 'general', id: 'menu-bar', label: 'Show in menu bar', keywords: 'system tray icon start' },
  { section: 'shortcuts', id: 'shortcut-quick', label: 'Quick Leo shortcut', keywords: 'keyboard hotkey keys bind search ask jot change' },
  { section: 'shortcuts', id: 'shortcut-new', label: 'New note shortcut', keywords: 'keyboard hotkey keys bind create change' },
  { section: 'shortcuts', id: 'shortcut-ask', label: 'Ask Felix shortcut', keywords: 'keyboard hotkey keys bind selection margin change' },
  { section: 'shortcuts', id: 'shortcut-dictation', label: 'Dictation shortcut (any app)', keywords: 'keyboard hotkey keys bind speak record' },
  { section: 'dictation', id: 'dictation-activation', label: 'Activation', keywords: 'hold tap press dictation shortcut' },
  { section: 'dictation', id: 'dictation-microphone', label: 'Microphone', keywords: 'dictation input device audio mic' },
  { section: 'dictation', id: 'dictation-words', label: 'Custom words', keywords: 'dictionary names terms spelling vocabulary dictation' },
  { section: 'dictation', id: 'dictation-fillers', label: 'Remove filler words', keywords: 'um uh like dictation clean up' },
  { section: 'dictation', id: 'dictation-sounds', label: 'Start and stop sounds', keywords: 'chime beep audio dictation' },
  { section: 'dictation', id: 'dictation-ai', label: 'AI writing for dictation', keywords: 'grammar tidy rewrite lists dictation writing ai' },
  { section: 'dictation', id: 'dictation-popup', label: 'Popup', keywords: 'live minimal off overlay window position top bottom' },
  { section: 'dictation', id: 'dictation-recent', label: 'Recent dictations', keywords: 'history clear copy last' },
  { section: 'models', id: 'speech-in-use', label: 'Speech model in use', keywords: 'whisper transcription recording dictation ready choose' },
  { section: 'models', id: 'speech-language', label: 'Speech language', keywords: 'detect language transcription' },
  { section: 'models', id: 'speech-downloaded', label: 'Downloaded speech models', keywords: 'delete disk space size local' },
  { section: 'models', id: 'speech-all', label: 'All speech models', keywords: 'download recommended english accuracy speed catalog filter' },
  { section: 'writing', id: 'writing-provider', label: 'Writing AI provider', keywords: 'ollama codex claude code felix notes model service' },
  { section: 'writing', id: 'writing-model', label: 'Writing AI model', keywords: 'effort price cost thinking' },
  { section: 'writing', id: 'writing-key', label: 'API key', keywords: 'secret token credential password' },
  { section: 'writing', id: 'writing-test', label: 'Test the AI', keywords: 'check connection working try' },
  { section: 'writing', id: 'writing-activity', label: 'Felix activity', keywords: 'usage tokens cache requests tools calls timing processes running codex claude' },
  { section: 'felix', id: 'felix-new-notes', label: 'New notes start', keywords: 'felix reads along read along when you ask margin mode privacy' },
  { section: 'felix', id: 'felix-persona', label: 'Who reads your notes', keywords: 'felix persona personas reader critic encourager style tone custom your own margin' },
  { section: 'felix', id: 'felix-where', label: 'Where notes appear', keywords: 'felix margin both right marks only layout' },
  { section: 'felix', id: 'felix-auto-model', label: 'Cheaper model for lookups', keywords: 'felix model routing auto cheap cheaper cost tokens lookup' },
  { section: 'felix', id: 'felix-pace', label: 'Reading pace', keywords: 'felix margin fast regular slow pause wait' },
  { section: 'felix', id: 'felix-gap', label: 'Time between reads', keywords: 'felix margin often seconds minutes 3 min wait cooldown' },
  { section: 'felix', id: 'felix-cap', label: 'Reads a day', keywords: 'felix margin often daily limit cap budget' },
  { section: 'calendar', id: 'calendar-connect', label: 'Connect a calendar', keywords: 'google ical ics outlook apple link secret address meetings classes' },
  { section: 'keeping', id: 'keeping-trash', label: 'Keep the trash', keywords: 'deleted notes days retention delete' },
  { section: 'keeping', id: 'keeping-chat', label: 'Keep chats with Felix', keywords: 'conversations days retention delete' },
  { section: 'keeping', id: 'keeping-upload', label: 'Keep uploaded files', keywords: 'originals documents pdf days retention delete' },
  { section: 'keeping', id: 'keeping-transcript', label: 'Keep recording transcripts', keywords: 'recordings sources what was said days retention delete' },
  { section: 'keeping', id: 'keeping-audio', label: 'Keep unfinished recording audio', keywords: 'interrupted recordings recover days retention delete' },
  { section: 'keeping', id: 'keeping-export', label: 'Export notes', keywords: 'zip download backup uploads chats trash storage disk' },
  { section: 'keeping', id: 'keeping-restore', label: 'Restore tasks', keywords: 'import tasks json bring back old computer' },
  { section: 'trash', id: 'trash-notes', label: 'Trash', keywords: 'deleted notes restore empty bin recover' },
  { section: 'health', id: 'health-checks', label: 'Health checks', keywords: 'doctor microphone permissions accessibility ready problems fix' },
  { section: 'updates', id: 'updates-check', label: 'Check for updates', keywords: 'new version download install restart' },
  { section: 'updates', id: 'updates-automatic', label: 'Check for updates automatically', keywords: 'hourly startup' },
  { section: 'updates', id: 'updates-on-quit', label: 'Install updates when I quit', keywords: 'automatic restart' },
  { section: 'updates', id: 'updates-about', label: 'About leo', keywords: 'version folder notes location models path terminal commands uninstall' },
];
const SETTINGS_ICON_PATHS = {
  sliders: '<path d="M4 6h10M4 12h4M12 12h8M4 18h12"/><circle cx="16.5" cy="6" r="2.2"/><circle cx="10" cy="12" r="2.2"/><circle cx="18.5" cy="18" r="2.2"/>',
  command: '<path d="M15 6v12a3 3 0 1 0 3-3H6a3 3 0 1 0 3 3V6a3 3 0 1 0-3 3h12a3 3 0 1 0-3-3"/>',
  keyboard: '<rect x="2.5" y="6" width="19" height="12" rx="2.5"/><path d="M6.5 10h.01M10 10h.01M14 10h.01M17.5 10h.01M7.5 14h9"/>',
  wave: '<path d="M4 10v4M8 7v10M12 4v16M16 8v8M20 10.5v3"/>',
  pen: '<path d="M4 20h4L18.5 9.5a2.8 2.8 0 0 0-4-4L4 16v4z"/><path d="M13 7l4 4"/>',
  pulse: '<path d="M3 12h4l2.5-6 5 12 2.5-6h4"/>',
  update: '<circle cx="12" cy="12" r="9"/><path d="M12 7.5v8M8.5 12.5l3.5 3.5 3.5-3.5"/>',
};
const settingsItems = () => SETTINGS_SECTIONS.flatMap((group) => group.items);
function settingsIcon(name) {
  return SETTINGS_ICON_PATHS[name] ? svg(SETTINGS_ICON_PATHS[name]) : ICON[name] || '';
}
function settingsSection(section) {
  if (!section) { try { section = localStorage.getItem('leo-settings-section') || 'general'; } catch (e) { section = 'general'; } }
  return settingsItems().some(([id]) => id === section) ? section : 'general';
}
function settingsTitle(section) {
  return settingsItems().find(([id]) => id === section)?.[1] || 'General';
}
let settingsDrawn = '', settingsPillFrom = null;
function settingsCrumbs(section) {
  const [, title] = settingsItems().find(([id]) => id === section) || settingsItems()[0];
  return `<span class="sep">/</span><button data-action="settings">Settings</button><span class="sep">/</span><button aria-current="page">${esc(title)}</button>`;
}

const settingsFind = { query: '', active: 0 };
function settingsMatches(query) {
  const words = String(query).toLowerCase().split(/\s+/).filter(Boolean);
  if (!words.length) return [];
  return SETTINGS_INDEX.flatMap((entry) => {
    const label = entry.label.toLowerCase();
    if (!words.every((word) => `${label} ${entry.keywords}`.includes(word))) return [];
    return [{ entry, rank: label.startsWith(words[0]) ? 0 : words.every((word) => label.includes(word)) ? 1 : 2 }];
  }).sort((a, b) => a.rank - b.rank).map((match) => match.entry);
}
function settingsResults() {
  const query = settingsFind.query.trim();
  if (!query) return { html: '', active: '', open: false };
  const found = settingsMatches(query);
  if (!found.length) return { html: `<p class="settings-empty" role="status">No settings match “${esc(query)}”.</p>`, active: '', open: false };
  const at = Math.min(settingsFind.active, found.length - 1);
  const options = found.map((entry, i) => `<div class="settings-result${i === at ? ' on' : ''}" role="option" id="settings-result-${i}" aria-selected="${i === at}" data-find-pick="${esc(entry.id)}"><span class="settings-result-label">${esc(entry.label)}</span><span class="settings-result-where">${esc(settingsTitle(entry.section))}</span></div>`).join('');
  return { html: `<div id="settings-listbox" role="listbox" aria-label="Settings search results">${options}</div>`, active: `settings-result-${at}`, open: true };
}
function paintSettingsSearch() {
  const nav = app.querySelector('.settings-nav');
  if (!nav) return;
  const results = settingsResults();
  const input = nav.querySelector('#settings-search');
  const box = nav.querySelector('.settings-results');
  const opening = box && results.open && !box.firstElementChild;
  if (box && box.innerHTML !== results.html) box.innerHTML = results.html;
  if (opening) cascade(box.firstElementChild);
  nav.toggleAttribute('data-searching', Boolean(settingsFind.query.trim()));
  if (!input) return;
  if (input.value !== settingsFind.query) input.value = settingsFind.query;
  input.setAttribute('aria-expanded', String(results.open));
  if (results.open) { input.setAttribute('aria-controls', 'settings-listbox'); input.setAttribute('aria-activedescendant', results.active); }
  else { input.removeAttribute('aria-controls'); input.removeAttribute('aria-activedescendant'); }
}
function moveSettingsResult(step) {
  const count = settingsMatches(settingsFind.query).length;
  if (!count) return;
  settingsFind.active = (Math.min(settingsFind.active, count - 1) + step + count) % count;
  paintSettingsSearch();
  app.querySelector('.settings-result.on')?.scrollIntoView({ block: 'nearest' });
}
function chooseSetting(id) {
  const entry = SETTINGS_INDEX.find((item) => item.id === id);
  if (!entry) return;
  settingsFind.query = '';
  settingsFind.active = 0;
  paintSettingsSearch();
  seekSettingsTarget(entry);
  if (state.view !== 'settings' || state.section !== entry.section) location.hash = `#/settings/${entry.section}`;
}
let settingsSeek = null;
function seekSettingsTarget(entry) {
  if (settingsSeek) settingsSeek();
  const section = `.settings-section[data-settings-section="${entry.section}"]`;
  let timer = 0;
  const observer = new MutationObserver(() => attempt(false));
  const stop = () => { observer.disconnect(); clearTimeout(timer); settingsSeek = null; };
  const attempt = (late) => {
    const page = app.querySelector(section);
    const target = page && (page.querySelector(`[data-find="${entry.id}"]`) || (late ? page.querySelector('.settings-head') : null));
    if (!target) return late ? stop() : false;
    stop();
    const arriving = typeof page.getAnimations === 'function' ? page.getAnimations() : [];
    Promise.allSettled(arriving.map((animation) => animation.finished)).then(() => requestAnimationFrame(() => arriveAtSetting(target)));
    return true;
  };
  if (attempt(false)) return;
  observer.observe(app, { childList: true, subtree: true });
  timer = setTimeout(() => attempt(true), 2500);
  settingsSeek = stop;
}
function arriveAtSetting(target) {
  if (!target.isConnected) return;
  const still = stillMotion();
  if (!target.hasAttribute('tabindex')) target.tabIndex = -1;
  target.scrollIntoView({ block: 'center' });
  target.focus({ preventScroll: true });
  target.classList.remove('found', 'found-still');
  target.classList.add(still ? 'found-still' : 'found');
  pulse(target, { ring: true });
  target.addEventListener('blur', () => target.classList.remove('found', 'found-still'), { once: true });
}
document.addEventListener('input', (e) => {
  if (!e.target.matches?.('#settings-search')) return;
  settingsFind.query = e.target.value;
  settingsFind.active = 0;
  paintSettingsSearch();
});
document.addEventListener('keydown', (e) => {
  if (!e.target.matches?.('#settings-search')) return;
  if (e.key === 'Escape' && settingsFind.query) {
    e.preventDefault();
    e.stopPropagation();
    settingsFind.query = '';
    paintSettingsSearch();
  } else if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
    e.preventDefault();
    moveSettingsResult(e.key === 'ArrowDown' ? 1 : -1);
  } else if (e.key === 'Enter') {
    const found = settingsMatches(settingsFind.query);
    const entry = found[Math.min(settingsFind.active, found.length - 1)];
    if (!entry) return;
    e.preventDefault();
    chooseSetting(entry.id);
  }
}, true);
document.addEventListener('click', (e) => {
  const pick = e.target.closest?.('[data-find-pick]');
  if (pick) chooseSetting(pick.dataset.findPick);
});
document.addEventListener('pointerdown', (e) => {
  if (e.target.closest?.('[data-find-pick]')) e.preventDefault();
});

function settingsFrame(section, content) {
  const shown = app.querySelector('.settings-section');
  const arrive = settingsDrawn !== section || !shown;
  const ids = settingsItems().map(([id]) => id);
  const from = ids.indexOf(settingsDrawn), to = ids.indexOf(section);
  const oldNav = shown && app.querySelector('.settings-sections'), oldPill = oldNav?.querySelector('.settings-nav-pill.placed');
  const box = oldPill?.getBoundingClientRect(), oldOn = oldNav?.querySelector('.settings-link.on')?.getBoundingClientRect();
  if (box && box.height && oldOn && Math.abs(box.top - oldOn.top) < 1) {
    const frame = oldNav.getBoundingClientRect();
    settingsPillFrom = { top: box.top - frame.top - oldNav.clientTop + oldNav.scrollTop, height: box.height };
  } else settingsPillFrom = null;
  settingsDrawn = section;
  const [, title, , about] = settingsItems().find(([id]) => id === section) || settingsItems()[0];
  const nav = SETTINGS_SECTIONS.map((group) => `<div class="settings-group"><div class="settings-group-title">${esc(group.group)}</div>${group.items.map(([id, label, icon]) => `<a class="settings-link${id === section ? ' on' : ''}" href="#/settings/${id}"${id === section ? ' aria-current="page"' : ''}>${settingsIcon(icon)}<span>${esc(label)}</span></a>`).join('')}</div>`).join('');
  const found = settingsResults();
  const search = `<div class="settings-search" role="search">${svg('<circle cx="11" cy="11" r="6.5"/><path d="M16 16l4.5 4.5"/>')}<input id="settings-search" type="text" role="combobox" aria-label="Search settings" aria-autocomplete="list" aria-expanded="${found.open}"${found.open ? ` aria-controls="settings-listbox" aria-activedescendant="${found.active}"` : ''} placeholder="Search settings" autocomplete="off" spellcheck="false" value="${esc(settingsFind.query)}"></div>`;
  // Callers assign the frame to the page right away; once they have, bring the current
  // section into view in the phone strip, where the list scrolls sideways.
  if (typeof queueMicrotask === 'function') queueMicrotask(revealSettingsLink);
  if (arrive && shown) overlapOut(shown);
  const lean = arrive && shown && from >= 0 && to >= 0 ? (to > from ? ' style="--arrive-from:6px"' : ' style="--arrive-from:-6px"') : '';
  return `<div class="settings-layout"><nav class="settings-nav" aria-label="Settings sections"${settingsFind.query.trim() ? ' data-searching' : ''}>${search}<div class="settings-lists"><div class="settings-sections"><span class="settings-nav-pill" aria-hidden="true"></span>${nav}</div><div class="settings-results" id="settings-results">${found.html}</div></div></nav><div class="settings-section${arrive ? ' arrive' : ''}" data-settings-section="${esc(section)}"${lean}><header class="settings-head"><h1>${esc(title)}</h1><p>${esc(about)}</p></header>${content}</div></div>`;
}

function placeSettingsPill(nav, on) {
  const pill = nav.querySelector('.settings-nav-pill');
  if (!pill || !on) return;
  const from = settingsPillFrom;
  settingsPillFrom = null;
  pill.style.transform = `translateY(${on.offsetTop}px)`;
  pill.style.height = `${on.offsetHeight}px`;
  pill.classList.add('placed');
  if (!from || Math.abs(from.top - on.offsetTop) < 1 || stillMotion()) return;
  pill.animate([{ transform: `translateY(${from.top}px)`, height: `${from.height}px` }, { transform: `translateY(${on.offsetTop}px)`, height: `${on.offsetHeight}px` }], { duration: DRAWER_MS, easing: EASE_MOVE });
}

const settingsRevealed = new WeakMap();
const settingsWatched = new WeakSet();
function watchSettingsNav(frame) {
  if (!frame || settingsWatched.has(frame) || typeof ResizeObserver !== 'function') return;
  settingsWatched.add(frame);
  const watcher = new ResizeObserver(() => {
    const sections = frame.querySelector('.settings-sections');
    if (!frame.isConnected || !sections) return watcher.disconnect();
    settingsRevealed.delete(sections);
    revealSettingsLink();
  });
  watcher.observe(frame);
}
function revealSettingsLink() {
  const nav = app.querySelector('.settings-sections'), on = nav?.querySelector('.settings-link.on');
  if (nav) placeSettingsPill(nav, on);
  watchSettingsNav(nav?.closest('.settings-nav'));
  if (!on || settingsRevealed.get(nav) === on.getAttribute('href')) return;
  settingsRevealed.set(nav, on.getAttribute('href'));
  if (nav.scrollWidth > nav.clientWidth) nav.scrollLeft = on.offsetLeft - (nav.clientWidth - on.offsetWidth) / 2;
}

const KNOB_STRETCH = 1.12;
function knobStretch(knob, pseudoElement) {
  if (!knob || stillMotion() || typeof knob.animate !== 'function') return;
  knob.animate({ scale: ['1 1', `${KNOB_STRETCH} 1`, '1 1'] }, { duration: DRAWER_MS, easing: EASE_MOVE, pseudoElement });
}

// An iOS-style switch. It stays a real checkbox, so change handlers, forms and tests keep working.
function settingsSwitch(attrs, on, label = '') {
  return `<input type="checkbox" class="switch" role="switch"${label ? ` aria-label="${esc(label)}"` : ''} ${attrs}${on ? ' checked' : ''}>`;
}

// A segmented control whose highlight slides to the picked option. Two kinds:
// - `field`: a form value. A hidden input carries `field` attributes and the value; picking one
//   updates it and fires `change` on it, like the <select> it replaces.
// - `action`: each option is a data-action button (the caller's handler stores the choice).
function segmented({ label, choices, current, field = '', action = '', key = '' }) {
  const options = choices.map(([value, text, icon = '', hint = '']) => {
    const on = String(value) === String(current);
    const what = action ? ` data-action="${action}" data-${key}="${esc(value)}" aria-pressed="${on}"` : ` data-action="segment-pick" data-value="${esc(value)}" role="radio" aria-checked="${on}" tabindex="${on ? 0 : -1}"`;
    return `<button type="button" class="segment${on ? ' on' : ''}"${what} title="${esc(hint || text)}">${icon}<span>${esc(text)}</span></button>`;
  }).join('');
  const index = Math.max(0, choices.findIndex(([value]) => String(value) === String(current)));
  const hidden = field ? `<input type="hidden" ${field} value="${esc(current ?? '')}">` : '';
  return `<div class="segmented" role="${action ? 'group' : 'radiogroup'}" aria-label="${esc(label)}" style="--count:${choices.length};--at:${index}">${hidden}<span class="segment-pill" aria-hidden="true"></span>${options}</div>`;
}
// Move the highlight to whichever option is on. `from` lets a redraw slide from where it was.
function syncSegmented(box, from = null) {
  const options = [...box.querySelectorAll('.segment')];
  const field = box.querySelector('input[type="hidden"]');
  if (field) for (const option of options) {
    const on = option.dataset.value === field.value;
    option.classList.toggle('on', on);
    option.setAttribute('aria-checked', String(on));
    option.tabIndex = on ? 0 : -1;
  }
  const at = Math.max(0, options.findIndex((option) => option.classList.contains('on')));
  const pill = box.querySelector('.segment-pill');
  if (from !== null && from !== at && pill) {
    pill.style.transition = 'none';
    box.style.setProperty('--at', from);
    void pill.offsetWidth;
    pill.style.transition = '';
  }
  if (Number(box.style.getPropertyValue('--at')) !== at) knobStretch(pill);
  box.style.setProperty('--at', at);
}
function pickSegment(el) {
  const box = el.closest('.segmented');
  const field = box?.querySelector('input[type="hidden"]');
  if (!field || field.value === el.dataset.value) return;
  field.value = el.dataset.value;
  syncSegmented(box);
  field.dispatchEvent(new Event('change', { bubbles: true }));
}
// Arrow keys move between options, as in a native radio group.
document.addEventListener('keydown', (e) => {
  const option = e.target.closest?.('.segment[role="radio"]');
  if (!option || !['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown', 'Home', 'End'].includes(e.key)) return;
  const options = [...option.parentElement.querySelectorAll('.segment')];
  const at = options.indexOf(option);
  const next = e.key === 'Home' ? 0 : e.key === 'End' ? options.length - 1 : (at + (/Left|Up/.test(e.key) ? -1 : 1) + options.length) % options.length;
  e.preventDefault();
  options[next].focus();
  pickSegment(options[next]);
});

let choiceCardsMade = 0;
function choiceCards({ label, choices, current, field = '', action = '', key = '', columns = 0, className = '' }) {
  const name = `choice-${++choiceCardsMade}`;
  const index = Math.max(0, choices.findIndex(([value]) => String(value) === String(current)));
  const cards = choices.map(([value, title, about = '', diagram = '', icon = ''], i) => {
    const on = i === index;
    const art = (diagram ? `<span class="choice-diagram" aria-hidden="true">${diagram}</span>` : '') + (icon ? `<span class="choice-icon">${icon}</span>` : '');
    const words = `<span class="choice-title">${esc(title)}</span>${about ? `<span class="choice-about">${esc(about)}</span>` : ''}`;
    return action
      ? `<button type="button" class="choice-pick${on ? ' on' : ''}" data-action="${action}" data-${key}="${esc(value)}" aria-pressed="${on}">${art}${words}</button>`
      : `<label class="choice-pick${on ? ' on' : ''}">${art}<input type="radio" name="${name}" value="${esc(value)}"${on ? ' checked' : ''}>${words}</label>`;
  }).join('');
  const hidden = field ? `<input type="hidden" ${field} value="${esc(current ?? '')}">` : '';
  return `<div class="choice-cards${className ? ` ${className}` : ''}" role="${action ? 'group' : 'radiogroup'}" aria-label="${esc(label)}" style="--count:${columns || choices.length};--at:${index}">${hidden}<span class="choice-ring" aria-hidden="true"></span>${cards}</div>`;
}
function syncChoiceCards(box, from = null) {
  const picks = [...box.querySelectorAll('.choice-pick')];
  const field = box.querySelector('input[type="hidden"]');
  if (field) for (const pick of picks) pick.querySelector('input').checked = pick.querySelector('input').value === field.value;
  for (const pick of picks) if (pick.querySelector('input')) pick.classList.toggle('on', pick.querySelector('input').checked);
  const at = Math.max(0, picks.findIndex((pick) => pick.classList.contains('on')));
  const ring = box.querySelector('.choice-ring');
  if (from !== null && from !== at && ring) {
    ring.style.transition = 'none';
    box.style.setProperty('--at', from);
    void ring.offsetWidth;
    ring.style.transition = '';
  }
  if (Number(box.style.getPropertyValue('--at')) !== at) knobStretch(ring);
  box.style.setProperty('--at', at);
}
document.addEventListener('change', (e) => {
  if (e.target.matches?.('.switch')) knobStretch(e.target, '::before');
  const radio = e.target.closest?.('.choice-pick input[type="radio"]');
  const box = radio?.closest('.choice-cards');
  if (!box) return;
  const field = box.querySelector('input[type="hidden"]');
  if (field) field.value = radio.value;
  syncChoiceCards(box);
  if (field) field.dispatchEvent(new Event('change', { bubbles: true }));
});

function settingReset(label, key, differs) {
  return `<button type="button" class="set-reset"${differs ? ' data-on' : ''} data-action="setting-reset" data-reset="${esc(key)}" aria-label="Reset ${esc(label)} to default" title="Reset to default">${svg('<path d="M4 9h11a5 5 0 0 1 0 10h-4"/><path d="M8 5L4 9l4 4"/>')}</button>`;
}

function keepSettingsFields() {
  return Array.from(app.querySelectorAll('input:not([type="checkbox"]):not([type="file"]),textarea,[data-dictation]')).map((field) => {
    const selector = field.id ? '#' + CSS.escape(field.id) : field.dataset.dictation ? `[data-dictation="${CSS.escape(field.dataset.dictation)}"]` : field.dataset.keyInput ? `[data-key-input="${CSS.escape(field.dataset.keyInput)}"]` : null;
    return { selector, value: field.value, checked: field.checked, start: field.selectionStart, end: field.selectionEnd, active: field === document.activeElement };
  }).filter((field) => field.selector);
}
function keepSegmented() {
  return new Map(Array.from(app.querySelectorAll('.segmented[aria-label],.choice-cards[aria-label]')).map((box) => [box.getAttribute('aria-label'), Number(box.style.getPropertyValue('--at')) || 0]));
}
function restoreSettingsFields(fields, segments = new Map()) {
  for (const saved of fields) {
    const field = app.querySelector(saved.selector);
    if (!field) continue;
    field.value = saved.value;
    if (field.type === 'checkbox') field.checked = saved.checked;
    if (saved.active) { field.focus({ preventScroll: true }); if (saved.start !== null && field.setSelectionRange && /^(text|textarea|search|password|url|tel)$/.test(field.type || 'text')) field.setSelectionRange(saved.start, saved.end); }
  }
  for (const box of app.querySelectorAll('.segmented')) syncSegmented(box, segments.get(box.getAttribute('aria-label')) ?? null);
  for (const box of app.querySelectorAll('.choice-cards')) syncChoiceCards(box, segments.get(box.getAttribute('aria-label')) ?? null);
}
