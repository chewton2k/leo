let desktopApp = null;
let appPollGeneration = 0;
let appPollTimer = 0;
let appPollVisible = null;
const APP_STATUS_ACTIVE_MS = 2500;
const APP_STATUS_IDLE_MS = 30000;
function scheduleAppStatus(generation) {
  if (generation !== appPollGeneration) return;
  clearTimeout(appPollTimer);
  appPollTimer = 0;
  if (!appVisible()) return;
  const active = state.view === 'settings' || ['checking', 'downloading', 'installing'].includes(desktopApp?.updates?.state);
  const delay = onboardingHere(0) ? ONBOARDING_POLL_MS : active ? APP_STATUS_ACTIVE_MS : APP_STATUS_IDLE_MS;
  appPollTimer = setTimeout(() => pollAppStatus(generation), delay);
}
function appVisibilityChanged() {
  const visible = appVisible();
  if (visible === appPollVisible) return;
  appPollVisible = visible;
  clearTimeout(appPollTimer);
  appPollTimer = 0;
  const generation = ++appPollGeneration;
  if (typeof checkActivity === 'function') checkActivity();
  if (!visible) return;
  pollAppStatus(generation);
  if (state.view === 'settings') {
    if (typeof startSpeechPolling === 'function') startSpeechPolling(seq);
    if (dictationSection(state.section) && typeof pollDictation === 'function') pollDictation(seq, ++dictationGeneration);
  }
}
function nativeGeneralCard(view) {
  if (!view) return '';
  const p = view.preferences || {};
  return `<section class="set-card"><header><h3>This app</h3></header><p class="set-note warn" id="app-preferences-problem"${view.problem ? '' : ' hidden'}>${esc(view.problem || '')}</p><label class="set-row" data-find="open-at-login"><span class="grow">Open at login<span class="set-desc">Start leo when you sign in to this computer.</span></span>${settingsSwitch('data-app-pref="open_at_login"', p.open_at_login)}</label><label class="set-row" data-find="menu-bar"><span class="grow">Show in ${view.platform === 'macos' ? 'menu bar' : 'system tray'}<span class="set-desc">Keep leo one click away while the window is closed.</span></span>${settingsSwitch('data-app-pref="show_tray"', p.show_tray)}</label></section>`;
}
function healthCards(view) {
  if (!view) return `<section class="set-card hc-card hc-loading" data-find="health-checks"><div class="hc-sum"><span class="activity-spin" aria-hidden="true"></span><div class="hc-sum-text"><b>Checking…</b><span>Microphone, speech model and AI on this computer.</span></div></div><div class="hc-tiles">${skeleton(6)}</div></section>`;
  const glyph = (kind, paths) => `<svg class="hc-g ${kind}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${paths}</svg>`;
  const WARN = '<path d="M10.3 4.2 2.6 17.6A2 2 0 0 0 4.3 20.6h15.4a2 2 0 0 0 1.7-3L13.7 4.2a2 2 0 0 0-3.4 0z"/><path d="M12 9.5v4"/><path d="M12 17h.01"/>';
  const MISSING = '<circle cx="12" cy="12" r="8.6"/><path d="M8.2 12h7.6"/>';
  const TICK = '<path d="M5 12.5l4.5 4.5L19 7.5"/>';
  const TILE_ICONS = {
    'microphone permission': '<rect x="9" y="3" width="6" height="11" rx="3"/><path d="M5 11a7 7 0 0 0 14 0M12 18v3"/>',
    'microphone': '<rect x="9" y="3" width="6" height="11" rx="3"/><path d="M5 11a7 7 0 0 0 14 0M12 18v3"/>',
    'microphone device': '<rect x="9" y="3" width="6" height="11" rx="3"/><path d="M5 11a7 7 0 0 0 14 0M12 18v3"/>',
    'accessibility permission': '<circle cx="12" cy="12" r="9"/><circle cx="12" cy="7.2" r="1.3"/><path d="M7.5 10.2l4.5 1 4.5-1M12 11.2v3.6l-2.2 3.7M12 14.8l2.2 3.7"/>',
    'speech model': '<path d="M4 10v4M8 7v10M12 4v16M16 8v8M20 10.5v3"/>',
    'dictation shortcut': '<rect x="2.5" y="6" width="19" height="12" rx="2.5"/><path d="M6.5 10h.01M10 10h.01M14 10h.01M17.5 10h.01M7.5 14h9"/>',
    'shortcut clash': '<rect x="2.5" y="6" width="19" height="12" rx="2.5"/><path d="M6.5 10h.01M10 10h.01M14 10h.01M17.5 10h.01M7.5 14h9"/>',
    'codex sign-in': '<path d="M4 20h4L18.5 9.5a2.8 2.8 0 0 0-4-4L4 16v4z"/><path d="M13 7l4 4"/>',
    'claude sign-in': '<path d="M4 20h4L18.5 9.5a2.8 2.8 0 0 0-4-4L4 16v4z"/><path d="M13 7l4 4"/>',
    'leo': '<circle cx="12" cy="12" r="9"/><path d="M12 7.5v8M8.5 12.5l3.5 3.5 3.5-3.5"/>',
    'leo on your path': '<rect x="3" y="4.5" width="18" height="15" rx="2.5"/><path d="M7.5 10l3 2.5-3 2.5M13 15h3.5"/>',
    'config file': '<path d="M4 6h10M4 12h4M12 12h8M4 18h12"/><circle cx="16.5" cy="6" r="2.2"/><circle cx="10" cy="12" r="2.2"/><circle cx="18.5" cy="18" r="2.2"/>',
    'app preferences': '<path d="M4 6h10M4 12h4M12 12h8M4 18h12"/><circle cx="16.5" cy="6" r="2.2"/><circle cx="10" cy="12" r="2.2"/><circle cx="18.5" cy="18" r="2.2"/>',
    'credentials': '<path d="M4 6h10M4 12h4M12 12h8M4 18h12"/><circle cx="16.5" cy="6" r="2.2"/><circle cx="10" cy="12" r="2.2"/><circle cx="18.5" cy="18" r="2.2"/>',
    'notes directory': '<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>',
    'notes': '<path d="M6 3h9l4 4v14H6z"/><path d="M9 12h7M9 16h5"/>',
    'duplicate note ids': '<path d="M6 3h9l4 4v14H6z"/><path d="M9 12h7M9 16h5"/>',
    'conflict copies': '<path d="M6 3h9l4 4v14H6z"/><path d="M9 12h7M9 16h5"/>',
    'unreadable notes': '<path d="M6 3h9l4 4v14H6z"/><path d="M9 12h7M9 16h5"/>',
    'reading along': '<path d="M6 3h9l4 4v14H6z"/><path d="M9 12h7M9 16h5"/>',
    'finding notes by meaning': '<circle cx="11" cy="11" r="7"/><path d="M20 20l-3.5-3.5"/>'
  };
  const name = (text) => { const t = String(text || ''); return /^leo/i.test(t) ? t : t.charAt(0).toUpperCase() + t.slice(1); };
  const isPath = (text) => /^(~|\/|[A-Za-z]:[\\/]|\\\\)/.test(String(text || '')) && /[\\/]/.test(String(text).slice(1));
  const rich = (text) => esc(text).replace(/`([^`]+)`/g, '<code>$1</code>');
  const checks = (view.sections || []).flatMap((section) => section.checks || []);
  const problems = checks.filter((check) => check.state !== 'ready').sort((a, b) => (a.state === 'missing' ? 0 : 1) - (b.state === 'missing' ? 0 : 1));
  const ready = checks.filter((check) => check.state === 'ready');
  const missing = problems.some((check) => check.state === 'missing');
  const kind = problems.length ? (missing ? 'missing' : 'warn') : 'ready';
  const headline = !problems.length ? `All ${checks.length} checks passed` : missing ? `${problems.length} ${problems.length === 1 ? 'check needs' : 'checks need'} you` : `${problems.length} ${problems.length === 1 ? 'check needs' : 'checks need'} a look`;
  const sub = `${problems.length ? `${ready.length} of ${checks.length} passed just now.` : 'Checked just now, on this computer.'} Your microphone stays closed and no AI request is sent.`;
  const head = `<div class="hc-sum" data-find="health-checks">${glyph(kind, kind === 'ready' ? TICK : kind === 'warn' ? WARN : MISSING)}<div class="hc-sum-text"><b>${esc(headline)}</b><span>${esc(sub)}</span></div><button class="btn plain sm" data-action="health-refresh">${ICON.refresh}Check again</button></div>`;
  const rows = problems.map((check) => `<div class="hc-fix ${check.state}">${glyph(check.state === 'warn' ? 'warn' : 'missing', check.state === 'warn' ? WARN : MISSING)}<b class="hc-nm">${esc(name(check.name))}</b><span class="hc-wd">${check.state === 'warn' ? 'Needs a look' : 'Needs setup'}</span>${check.detail && check.detail !== check.fix ? `<span class="hc-dt-line">${esc(check.detail)}</span>` : ''}${check.fix ? `<p class="health-fix">${rich(check.fix)}</p>` : ''}${check.action || check.section ? `<div class="hc-acts">${check.action ? `<button class="btn sm ink" data-action="app-command" data-app-action="${esc(check.action)}">${check.action === 'microphone' && desktopApp?.permissions?.microphone === 'denied' ? (desktopApp.platform === 'windows' ? 'Open Settings' : 'Open System Settings') : 'Allow access'}</button>` : ''}${check.section ? `<a class="btn plain sm" href="#/settings/${esc(check.section)}">Open ${esc(settingsTitle(check.section))}</a>` : ''}</div>` : ''}</div>`).join('');
  const tiles = ready.map((check) => {
    const label = name(check.name), wide = isPath(check.detail);
    const detail = !check.detail ? '<span class="hc-dt">Ready</span>' : wide ? (() => { const text = String(check.detail), tail = text.split(/(?=[\\/])/).slice(-1)[0]; return `<span class="hc-mid" title="${esc(text)}"><span class="h">${esc(text.slice(0, text.length - tail.length))}</span><span class="t">${esc(tail)}</span></span><span class="hc-sr">Ready</span>`; })() : `<span class="hc-dt" title="${esc(check.detail)}">${esc(check.detail)}</span><span class="hc-sr">Ready</span>`;
    return `<div class="hc-tile${wide ? ' wide' : ''}"><svg class="hc-ic" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${TILE_ICONS[String(check.name || '').toLowerCase()] || '<path d="M3 12h4l3-8 4 16 3-8h4"/>'}</svg><span class="hc-tn" title="${esc(label)}">${esc(label)}</span><span class="hc-ts">${glyph('ready', TICK)}${detail}</span></div>`;
  }).join('');
  return `<section class="set-card hc-card">${head}${rows ? `<div class="hc-needs">${rows}</div>` : ''}${tiles ? `<div class="hc-tiles">${tiles}</div>` : ''}</section>`;
}
async function loadHealth() {
  const mine = seq;
  const button = document.querySelector('[data-action="health-refresh"]');
  const label = button ? button.innerHTML : '';
  if (button) { button.disabled = true; button.setAttribute('aria-busy', 'true'); button.textContent = 'Checking…'; }
  try {
    const view = await api('/api/health');
    if (mine !== seq || state.view !== 'settings' || state.section !== 'health') return;
    state.health = view;
    const box = $('#app-health');
    refreshPanel(box, healthCards(view));
  } finally {
    const now = document.querySelector('[data-action="health-refresh"]');
    if (now && now.getAttribute('aria-busy') === 'true') { now.disabled = false; now.removeAttribute('aria-busy'); now.innerHTML = label || `${ICON.refresh}Check again`; }
  }
}
async function appChange(change) {
  const generation = ++appPollGeneration;
  try {
    const view = await api('/api/app', { method: 'POST', body: change });
    if (generation !== appPollGeneration) return;
    desktopApp = view;
    updateAppStatus();
    if (state.view === 'settings' && state.section === 'health') loadHealth().catch(fail);
    return view;
  } finally { scheduleAppStatus(generation); }
}

let updateChecking = false;
function updateAppStatus() {
  updateOnboardingPermissions();
  updateUpdateBanner();
  const checked = updateChecking && desktopApp?.updates?.state === 'current';
  updateChecking = desktopApp?.updates?.state === 'checking';
  if (state.view === 'settings' && state.section === 'updates') {
    const box = $('#app-updates');
    refreshPanel(box, updatesCard(desktopApp));
    if (checked) pulse(box.querySelector('.update-status'), { ring: true });
  }
  if (state.view === 'settings' && state.section === 'general') {
    const box = $('#app-general');
    if (box && !box.querySelector('[data-app-pref]')) refreshPanel(box, nativeGeneralCard(desktopApp));
    const problem = $('#app-preferences-problem');
    if (problem) { problem.hidden = !desktopApp?.problem; problem.textContent = desktopApp?.problem || ''; }
    for (const field of app.querySelectorAll('[data-app-pref]')) field.checked = Boolean(desktopApp?.preferences?.[field.dataset.appPref]);
  }
}
async function pollAppStatus(generation) {
  if (generation !== appPollGeneration || !appVisible()) return;
  try {
    const view = await api('/api/app');
    if (generation !== appPollGeneration) return;
    desktopApp = view;
    updateAppStatus();
    maybeOnboarding();
  } catch (e) {}
  scheduleAppStatus(generation);
}
app.addEventListener('change', (event) => {
  const field = event.target.closest('[data-app-pref]');
  if (field) appChange({ action: 'preference', field: field.dataset.appPref, value: field.checked }).then(() => pulse(field)).catch(fail);
});
document.addEventListener('visibilitychange', appVisibilityChanged);
if (typeof window !== 'undefined' && window.addEventListener) {
  window.addEventListener('DOMContentLoaded', appVisibilityChanged, { once: true });
  window.addEventListener('leo-visibility', appVisibilityChanged);
}


function middlePath(path) {
  const text = String(path || '');
  const tail = text.split(/(?=[\\/])/).slice(-2).join('');
  return `<span class="set-path" title="${esc(text)}"><span class="set-path-head">${esc(text.slice(0, text.length - tail.length))}</span><span class="set-path-tail">${esc(tail)}</span></span>`;
}
function updatesCard(view) {
  if (!view) return '<section class="set-card"><p class="hint">Open the leo app to check for updates.</p></section>';
  const preferences = view.preferences || {}, update = view.updates || {};
  const busy = update.busy;
  const status = { idle: 'Check for a newer version.', checking: 'Checking…', current: 'leo is up to date.', available: 'A new version is available.', downloading: 'Downloading…', ready: 'The update is ready.', installing: 'Installing…', error: 'Check again when you are connected.' }[update.state] || '';
  const checked = update.last_checked ? `Last checked ${new Date(Number(update.last_checked) * 1000).toLocaleString([], { dateStyle: 'medium', timeStyle: 'short' })}` : 'Not checked yet';
  const pct = update.total ? Math.min(100, Math.floor(update.bytes * 100 / update.total)) : null;
  const progress = update.state === 'downloading' ? `<div class="onboarding-download update-download"><span class="hint">${pct === null ? speechBytes(update.bytes) : `${pct}% · ${speechBytes(update.bytes)} of ${speechBytes(update.total)}`}</span>${pct === null ? '' : `<progress value="${pct}" max="100" aria-label="Downloading leo ${esc(update.version || '')}"></progress>`}</div>` : '';
  const restart = update.state === 'ready' ? `<button class="btn primary sm" data-action="app-command" data-app-action="restart_update"${busy ? ' disabled' : ''}>Restart to update</button>` : '';
  const download = update.state === 'available' || update.state === 'error' && update.version ? update.state === 'available' ? `<button class="btn primary sm" data-action="app-command" data-app-action="download_update">Download ${esc(update.version || '')}</button>` : '<button class="btn plain sm" data-action="app-command" data-app-action="download_update">Download</button>' : '';
  const unsigned = view.platform === 'macos' && view.unsigned ? `<p class="set-notice" role="note">${svg('<path d="M12 4 21.5 20h-19z"/><path d="M12 10v4.5M12 17.5h.01"/>')}<span>Unsigned copy: after an update, macOS may ask you to add leo again in Privacy &amp; Security → Accessibility.</span></p>` : '';
  return `<section class="set-card" data-find="updates-check"><header><h3>Updates <span class="set-version">${esc(view.version)}</span></h3></header><div class="set-row update-status"><span class="grow"><span class="update-state">${esc(update.state === 'idle' ? checked : status)}</span>${update.state === 'idle' ? '' : `<span class="hint">${esc(checked)}</span>`}</span><span class="buttons">${download}${restart}${update.state === 'available' ? '' : `<button class="btn plain sm" data-action="app-command" data-app-action="check_updates"${['checking','downloading','installing'].includes(update.state) ? ' disabled' : ''}>Check now</button>`}</span></div>${busy && restart ? '<p class="hint">Finish recording or dictation first.</p>' : ''}${progress}${update.error ? `<p class="set-note warn">${esc(update.error)}</p>` : ''}
    <label class="set-row" data-find="updates-automatic"><span class="grow">Check for updates automatically<span class="set-desc">Look for a newer version in the background.</span></span>${settingsSwitch('data-app-pref="automatic_updates"', preferences.automatic_updates)}</label><label class="set-row" data-find="updates-on-quit"><span class="grow">Install updates when I quit leo<span class="set-desc">Apply a downloaded update the next time leo closes.</span></span>${settingsSwitch('data-app-pref="install_on_quit"', preferences.install_on_quit)}</label><p class="set-note">Updates wait for recording and dictation to finish, then leo reopens.</p></section>
    ${update.notes ? `<section class="set-card"><header><h3>What’s new${update.version ? ' in ' + esc(update.version) : ''}</h3></header><div class="prose">${md.render(Array.from(String(update.notes)).slice(0, 8192).join(''), { interactive: false })}</div></section>` : ''}
    <section class="set-card about" data-find="updates-about"><header><h3>About leo</h3><a class="set-link" href="https://github.com/chewton2k/leo-cli/releases" target="_blank" rel="noopener noreferrer">Release notes</a></header>${unsigned}<div class="set-row"><span class="set-label">Notes</span>${middlePath(view.paths?.notes)}</div><div class="set-row"><span class="set-label">Speech models</span>${middlePath(view.paths?.models)}</div><div class="set-row"><code class="set-label">leo update</code><span class="hint">Installs the latest app</span></div><div class="set-row"><code class="set-label">leo uninstall</code><span class="hint">Removes leo; your notes stay</span></div></section>`;
}
function updateUpdateBanner() {
  const banner = $('#app-update-banner');
  if (!banner) return;
  const update = desktopApp?.updates || {};
  const ready = update.state === 'ready';
  banner.hidden = !ready;
  if (!ready) return;
  if (banner.dataset.version !== update.version) {
    banner.dataset.version = update.version;
    banner.innerHTML = `<span class="grow">leo ${esc(update.version)} is ready</span><button class="btn primary sm" data-action="app-command" data-app-action="restart_update">Restart to update</button><span class="hint" data-update-wait></span>`;
  }
  const button = banner.querySelector('button');
  if (button) button.disabled = Boolean(update.busy);
  const wait = banner.querySelector('[data-update-wait]');
  if (wait) wait.textContent = update.busy ? 'After recording or dictation finishes.' : '';
}
