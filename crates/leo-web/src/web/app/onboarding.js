function nextOnboarding(preferences) {
  const done = preferences.onboarding || [false, false, false];
  return !done[0] || !done[1] ? 0 : !done[2] ? 2 : preferences.writing_done === false ? 3 : -1;
}
let onboardingShown = false;
let onboardingGeneration = 0;
let onboardingSpeech = null;
let onboardingChoice = '';
let onboardingFinishing = false;
let onboardingBusy = false;
let onboardingAll = false;
let onboardingAutoUse = '';
let onboardingWriting = null;
let onboardingWriter = '';
let onboardingOllama = null;
let onboardingOllamaTimer = 0;
let onboardingOllamaPick = '';
const ONBOARDING_POLL_MS = 1000;
const ONBOARDING_STEPS = [['access', 'Access'], ['model', 'Speech model'], ['writing', 'Writing AI']];
const ONBOARDING_WRITERS = ['claude_code', 'codex', 'ollama'];
function onboardingHere(step) { return document.querySelector(`[data-onboarding-step="${step}"]`); }
function onboardingStepper(step) {
  const at = [0, 0, 1, 2][step];
  return `<ol class="onboarding-steps" aria-label="Setup, step ${at + 1} of ${ONBOARDING_STEPS.length}">${ONBOARDING_STEPS.map(([id, label], index) => `<li class="${index < at ? 'done' : index === at ? 'on' : ''}"${index === at ? ' aria-current="step"' : ''}><span class="onboarding-step-dot">${index < at ? ICON.check : index + 1}</span>${label}</li>`).join('')}</ol>`;
}
// What each permission row shows. A denied microphone cannot be asked again on
// macOS (the system ignores a second request), so its button opens Settings.
function onboardingPermissionState(permission) {
  if (permission === 'microphone') {
    const value = desktopApp?.permissions?.microphone;
    return value === 'allowed' ? 'allowed' : value === 'denied' ? 'denied' : value === 'unavailable' ? 'unavailable' : 'ask';
  }
  return desktopApp?.platform === 'windows' || desktopApp?.permissions?.accessibility === true ? 'allowed' : 'ask';
}
// Continue needs the microphone (or a microphone state the system will not
// report); Accessibility only adds automatic pasting, so it stays optional.
function onboardingCanContinue() {
  const microphone = desktopApp?.permissions?.microphone;
  return microphone === 'allowed' || microphone === 'unavailable' || microphone === 'unknown';
}
function onboardingPermissionRow(permission, icon, name, why) {
  return `<div class="set-row onboarding-permission" data-onboarding-permission="${permission}"><span class="onboarding-icon">${icon}</span><div class="grow"><strong>${name}</strong><p class="hint">${why}</p><p class="hint onboarding-denied" hidden>leo is turned off in ${desktopApp?.platform === 'windows' ? '' : 'System '}Settings. Turn it on there, then come back.</p></div><div class="onboarding-permission-action"><button class="btn plain sm" data-action="app-command" data-app-action="${permission}">Allow ${permission === 'microphone' ? 'microphone' : 'Accessibility'}</button><span role="status" class="set-status" hidden></span></div></div>`;
}
function onboardingPermissionsPage() {
  const windows = desktopApp.platform === 'windows';
  return `<section class="set-card onboarding-card">${onboardingPermissionRow('microphone', ICON.mic, 'Microphone', 'For dictation and recording. leo opens it only while you capture speech.')}${windows ? '' : onboardingPermissionRow('accessibility', ICON.access, 'Accessibility', 'Pastes dictated words into the app you are using. Without it, you paste from the clipboard yourself.')}</section>`;
}
function maybeOnboarding(force = false, only = null) {
  if (!desktopApp || !desktopApp.preferences || !force && onboardingShown || document.querySelector('.scrim')) return;
  const step = only ?? nextOnboarding(desktopApp.preferences);
  if (step < 0) return;
  onboardingShown = true;
  const permissions = step === 0, writing = step === 3;
  const title = permissions ? 'Allow access' : writing ? 'Choose who writes your notes' : 'Choose a speech model';
  const description = permissions ? 'leo needs a couple of permissions for dictation and recording. Each one is checked off here as soon as you allow it.' : writing ? 'Notes, Felix and AI dictation use this AI. <span id="onboarding-found">leo found these on this computer.</span> You can change it later in Settings.' : 'Dictation and recording share one model that runs on this computer, so audio never leaves it. You can change it later in Settings.';
  const foot = permissions
    ? `<button class="btn plain" data-action="onboarding-skip">Skip setup</button><button class="btn primary" id="onboarding-continue" data-action="onboarding-next" data-step="0" disabled>Continue</button>`
    : writing ? `<button class="btn plain" data-action="onboarding-next" data-step="3">Skip for now</button><button class="btn primary" id="onboarding-use" data-action="onboarding-use" disabled>Use writing AI</button>`
    : `<button class="btn plain" data-action="onboarding-back">Back</button><button class="btn plain" data-action="onboarding-next" data-step="2">Skip for now</button>`;
  const body = permissions ? onboardingPermissionsPage() : writing ? '<section id="onboarding-writing" class="set-card onboarding-card"><p class="hint">Looking for writing AIs on this computer…</p></section>' : '<div id="onboarding-models" class="onboarding-models"><p class="hint">Opening the model library…</p></div>';
  const later = permissions ? 'You can finish anything you skip in Settings → Health.' : writing ? 'You can choose a writing AI later in Settings → Writing AI.' : 'You can choose a model later in Settings → Speech models.';
  sheet(`<div class="onboarding" data-onboarding-step="${step}">${onboardingStepper(step)}<h3>${title}</h3><p>${description}</p>${body}<p class="hint onboarding-later">${later}</p>${writing ? '<p class="hint onboarding-dictate" hidden></p>' : ''}<div class="buttons onboarding-foot">${foot}</div></div>`, { modal: true });
  updateOnboardingPermissions();
  if (step === 2) loadOnboardingModels(++onboardingGeneration);
  if (writing) { loadOnboardingWriting(++onboardingGeneration); dictateHint($('.onboarding-dictate')); }
}
function updateOnboardingPermissions() {
  if (!desktopApp) return;
  for (const permission of ['microphone', 'accessibility']) {
    const row = document.querySelector(`[data-onboarding-permission="${permission}"]`);
    if (!row) continue;
    const now = onboardingPermissionState(permission);
    const allowed = now === 'allowed';
    const button = row.querySelector('button');
    button.hidden = allowed || now === 'unavailable';
    const settings = desktopApp.platform === 'windows' ? 'Settings' : 'System Settings';
    const label = now === 'denied' ? `Open ${settings}` : `Allow ${permission === 'microphone' ? 'microphone' : 'Accessibility'}`;
    if (button.textContent !== label) button.textContent = label;
    button.setAttribute('aria-label', now === 'denied' ? `Open ${settings} for ${permission === 'microphone' ? 'the microphone' : 'Accessibility'}` : `Allow ${permission === 'microphone' ? 'microphone' : 'Accessibility'}`);
    row.querySelector('.onboarding-denied').hidden = now !== 'denied';
    row.classList.toggle('allowed', allowed);
    const status = row.querySelector('[role="status"]');
    status.hidden = !allowed && now !== 'unavailable';
    status.classList.toggle('ok', allowed);
    status.classList.toggle('missing', now === 'unavailable');
    const text = allowed ? 'Allowed' : now === 'unavailable' ? 'Off in this session' : '';
    if (status.dataset.shown !== text) { status.innerHTML = allowed ? `<span class="pencil-tick${status.dataset.shown === '' ? ' writing' : ''}" aria-hidden="true"></span>Allowed` : text; status.dataset.shown = text; }
  }
  const next = $('#onboarding-continue');
  if (next) next.disabled = !onboardingCanContinue() || onboardingFinishing;
}
async function finishOnboarding(el) {
  if (onboardingFinishing) return;
  const generation = ++onboardingGeneration, step = Number(el.dataset.step);
  onboardingFinishing = true;
  updateOnboardingPermissions();
  try {
    for (const index of step < 0 ? [0, 1, 2, 3] : step < 2 ? [0, 1] : [step]) {
      await appChange({ action: 'finish_step', step: index });
      if (generation !== onboardingGeneration || !onboardingHere(step < 0 ? 0 : step)) return;
    }
    closeSheet(true);
    maybeOnboarding(true);
  } finally { onboardingFinishing = false; updateOnboardingPermissions(); }
}
function skipOnboarding() { return finishOnboarding({ dataset: { step: '-1' } }); }
function onboardingBack() {
  if (!onboardingHere(2)) return;
  onboardingGeneration++;
  closeSheet(true);
  maybeOnboarding(true, 0);
}
async function loadOnboardingModels(generation) {
  try {
    const view = await api('/api/speech');
    if (generation !== onboardingGeneration || !onboardingHere(2)) return;
    onboardingSpeech = view;
    const models = view.models || [];
    if (!models.some((model) => model.id === onboardingChoice)) onboardingChoice = (models.find((model) => model.in_use) || models.find((model) => model.recommended) || models[0])?.id || '';
    drawOnboardingModels();
    if (models.some((model) => model.progress?.state === 'downloading')) setTimeout(() => loadOnboardingModels(generation), 700);
    else if (onboardingAutoUse) {
      const ready = onboardingAutoUse === onboardingChoice && models.find((model) => model.id === onboardingAutoUse)?.verified;
      onboardingAutoUse = '';
      if (ready) await onboardingModel({ dataset: { verb: 'use' } });
    }
  } catch (e) {
    if (generation !== onboardingGeneration || !onboardingHere(2)) return;
    const box = $('#onboarding-models');
    if (box) box.innerHTML = '<p class="set-note warn">The model library could not be opened. Skip for now and choose one in Settings → Speech models.</p>';
  }
}
function onboardingPercent(model) {
  const progress = model.progress || {};
  return Math.min(100, Math.floor((progress.bytes || 0) * 100 / Math.max(1, progress.total || model.size)));
}
function onboardingModelState(model) {
  return model.progress?.state === 'downloading' ? `<span class="chip accent">Downloading ${onboardingPercent(model)}%</span>` : model.in_use ? '<span class="chip accent">In use</span>' : model.verified ? '<span class="chip">Downloaded</span>' : '';
}
function onboardingModelCard(model) {
  const on = model.id === onboardingChoice;
  const names = (model.languages || []).map(speechLanguage);
  return `<button type="button" class="onboarding-model${on ? ' on' : ''}" data-action="onboarding-pick" data-model="${esc(model.id)}" aria-pressed="${on}"><span class="onboarding-model-head"><b>${esc(model.name)}</b>${model.recommended ? '<span class="chip">Recommended</span>' : ''}<span class="onboarding-model-state">${onboardingModelState(model)}</span></span>${model.description ? `<span class="hint">${esc(model.description)}</span>` : ''}<span class="hint" title="${esc(names.join(', '))}">${esc(names.length === 1 ? names[0] + ' only' : names.length + ' languages')} · ${speechBytes(model.size)}${model.streaming ? ' · Streaming' : ''}</span>${speechRatings(model)}</button>`;
}
function drawOnboardingModels() {
  const box = $('#onboarding-models');
  const view = onboardingSpeech;
  if (!box || !view) return;
  const models = view.models || [];
  if (!models.length) { box.innerHTML = '<p class="set-note">No speech models are available right now. Skip for now and choose one in Settings → Speech models.</p>'; return; }
  const recommended = models.filter((model) => model.recommended);
  const all = onboardingAll || !recommended.length;
  const shown = all ? models : recommended;
  const toggle = recommended.length && recommended.length < models.length ? `<div class="model-filters">${segmented({ label: 'Which models to show', action: 'onboarding-models', key: 'all', current: String(all), choices: [['false', 'Recommended'], ['true', `All ${models.length}`]] })}</div>` : '';
  const key = (all ? 'all:' : 'recommended:') + shown.map((model) => model.id).join(',');
  // Polling during a download only touches the state chips and the progress
  // bar, so a click is never lost to a button being replaced under the pointer.
  if (box.dataset.list === key) {
    for (const model of shown) {
      const chip = box.querySelector(`.onboarding-model[data-model="${CSS.escape(model.id)}"] .onboarding-model-state`);
      const html = onboardingModelState(model);
      if (chip && chip.dataset.shown !== html) { chip.innerHTML = html; chip.dataset.shown = html; }
    }
    drawOnboardingProgress();
    return;
  }
  const focused = document.activeElement && box.contains(document.activeElement) ? document.activeElement.dataset.all : null;
  box.dataset.list = key;
  box.innerHTML = `${toggle}<div class="onboarding-model-list" role="group" aria-label="Speech models">${shown.map(onboardingModelCard).join('')}</div><div id="onboarding-model-progress" class="onboarding-model-action"></div>`;
  drawOnboardingProgress();
  if (focused) box.querySelector(`[data-action="onboarding-models"][data-all="${focused}"]`)?.focus({ preventScroll: true });
}
function drawOnboardingProgress() {
  const model = onboardingSpeech?.models.find((model) => model.id === onboardingChoice);
  const box = $('#onboarding-model-progress');
  if (!box) return;
  if (!model) { box.innerHTML = '<span class="grow hint">Choose a model above.</span>'; return; }
  const progress = model.progress;
  const busy = onboardingBusy ? ' disabled' : '';
  if (progress?.state === 'downloading') {
    const percent = onboardingPercent(model);
    const text = `Downloading ${model.name} · ${speechBytes(progress.bytes)} of ${speechBytes(progress.total || model.size)}`;
    const bar = box.querySelector('progress');
    if (bar && box.dataset.model === model.id) {
      bar.value = percent;
      box.querySelector('.onboarding-download .hint').textContent = text;
      box.querySelector('button').disabled = onboardingBusy;
      return;
    }
    box.dataset.model = model.id;
    box.innerHTML = `<div class="grow onboarding-download"><span class="hint">${esc(text)}</span><progress value="${percent}" max="100" aria-label="Downloading ${esc(model.name)}"></progress></div><button class="btn plain" data-action="onboarding-model" data-verb="cancel"${busy}>Cancel</button>`;
    return;
  }
  box.dataset.model = '';
  const error = progress?.error && progress.state !== 'cancelled' ? `<p class="set-note warn">${esc(progress.error)}</p>` : '';
  const verb = model.verified ? 'use' : 'download';
  const label = model.verified ? 'Use and finish' : `Download and use · ${speechBytes(model.size)}`;
  box.innerHTML = `<span class="grow hint">${model.verified ? `${esc(model.name)} is ready on this computer.` : `${esc(model.name)} downloads once, then works offline.`}</span><button class="btn primary" data-action="onboarding-model" data-verb="${verb}"${busy}>${label}</button>${error}`;
}
function pickOnboardingModel(el) {
  if (!onboardingHere(2) || el.dataset.model === onboardingChoice) return;
  onboardingChoice = el.dataset.model;
  for (const card of document.querySelectorAll('.onboarding-model')) {
    const on = card.dataset.model === onboardingChoice;
    card.classList.toggle('on', on);
    card.setAttribute('aria-pressed', String(on));
  }
  drawOnboardingProgress();
}
function showOnboardingModels(el) {
  onboardingAll = el.dataset.all === 'true';
  drawOnboardingModels();
}
async function onboardingModel(el) {
  const model = onboardingSpeech?.models.find((model) => model.id === onboardingChoice);
  if (!model || onboardingBusy) return;
  const generation = ++onboardingGeneration;
  const verb = el.dataset.verb;
  onboardingAutoUse = verb === 'download' ? model.id : '';
  onboardingBusy = true;
  drawOnboardingProgress();
  try {
    const view = await api('/api/speech', { method: 'POST', body: { action: verb, id: model.id, language: model.languages.length === 1 ? model.languages[0] : speechUseLanguage(model) } });
    if (generation !== onboardingGeneration || !onboardingHere(2)) return;
    onboardingSpeech = view;
    if (verb === 'use') { onboardingBusy = false; await finishOnboarding({ dataset: { step: '2' } }); return; }
    drawOnboardingModels();
    if (verb === 'download') loadOnboardingModels(generation);
  } finally {
    onboardingBusy = false;
    if (onboardingHere(2)) drawOnboardingProgress();
  }
}
async function loadOnboardingWriting(generation) {
  try {
    const view = await api('/api/writing?probe=signin');
    if (generation !== onboardingGeneration || !onboardingHere(3)) return;
    onboardingWriting = view;
    const candidates = view.candidates || [], none = !candidates.some((c) => c.installed);
    onboardingWriter = ONBOARDING_WRITERS.find((id) => onboardingWriterReady(onboardingWriterCandidate(id))) || ONBOARDING_WRITERS.find((id) => onboardingWriterUsable(onboardingWriterCandidate(id))) || (none && onboardingWriterCandidate('ollama') && 'ollama') || ONBOARDING_WRITERS.find((id) => onboardingWriterCandidate(id)) || '';
    if (none && onboardingWriter === 'ollama' && $('#onboarding-found')) $('#onboarding-found').textContent = 'None of these is on this computer yet. Ollama runs here and needs no account.';
    const box = $('#onboarding-writing');
    if (box) box.innerHTML = `${choiceCards({ label: 'Writing AI', choices: ONBOARDING_WRITERS.filter((id) => candidates.some((c) => c.id === id)).map((id) => providerCard({ id })), current: onboardingWriter, action: 'onboarding-writer', key: 'writer', className: 'providers' })}<div id="onboarding-writer-status" role="status"></div>`;
    drawOnboardingWriting();
    if (onboardingWriter === 'ollama') loadOnboardingOllama(generation);
  } catch (e) {
    if (generation !== onboardingGeneration || !onboardingHere(3)) return;
    const box = $('#onboarding-writing');
    if (box) box.innerHTML = '<p class="set-note warn">leo could not look for writing AIs. Skip for now and choose one in Settings → Writing AI.</p>';
  }
}
function onboardingWriterCandidate(id) { return onboardingWriting?.candidates?.find((c) => c.id === id); }
function onboardingWriterReady(c) { return c?.id === 'ollama' ? !!(c.running && c.models?.length) : c?.signed_in === true; }
function onboardingWriterUsable(c) { return onboardingWriterReady(c) || (c?.id !== 'ollama' && !!c?.installed && c.signed_in == null); }
function onboardingWriterStatus(c) {
  if (!c) return '';
  const name = providerCard(c)[1], where = ON_MAC ? 'this Mac' : 'this computer';
  const row = (text, label, ok = false) => `<div class="set-row"><span class="grow">${text}</span>${ok ? '' : signInControl(c, 'onboarding-sign-in')}<span class="set-status ${ok ? 'ok' : 'missing'}">${label}</span></div>`;
  if (onboardingWriterReady(c)) return c.id === 'ollama' ? row(`Ollama is running on ${where}.`, 'Ready', true) + ollamaSetup(onboardingOllama, onboardingOllamaPick) : row(`${name} is signed in on ${where}.`, 'Ready', true);
  if (onboardingWriterUsable(c)) return row(`${name} is installed on ${where}. leo couldn’t tell whether you’re signed in.`, 'Sign-in unknown');
  if (c.id !== 'ollama') return c.installed ? row(signInLate(c.id) ? SIGN_IN_LATE : `Sign in to ${name}, then come back.`, 'Sign in needed') : row(`${name} is not installed on ${where}.${inApp(c.id)}`, 'Not installed');
  if (!c.installed) return ollamaSetup({ installed: false }, '');
  if (!c.running) return row('Open Ollama to start it.', 'Not running');
  return row('Choose a model for Ollama to download.', 'No model') + ollamaSetup(onboardingOllama, '');
}
function drawOnboardingWriting() {
  const c = onboardingWriterCandidate(onboardingWriter);
  refreshPanel($('#onboarding-writer-status'), onboardingWriterStatus(c));
  const use = $('#onboarding-use');
  if (!use) return;
  use.textContent = c ? `Use ${providerCard(c)[1]}` : 'Use writing AI';
  use.disabled = !onboardingWriterUsable(c) || onboardingBusy;
}
function pickOnboardingWriter(el) {
  if (!onboardingHere(3) || el.dataset.writer === onboardingWriter) return;
  const box = el.closest('.choice-cards'), picks = [...box.querySelectorAll('.choice-pick')];
  const from = picks.findIndex((pick) => pick.classList.contains('on'));
  onboardingWriter = el.dataset.writer;
  for (const pick of picks) { pick.classList.toggle('on', pick === el); pick.setAttribute('aria-pressed', String(pick === el)); }
  syncChoiceCards(box, from);
  drawOnboardingWriting();
  if (onboardingWriter === 'ollama') loadOnboardingOllama(onboardingGeneration);
}
function onboardingSignIn(el) {
  const generation = onboardingGeneration, provider = el.dataset.provider;
  return signIn(el, () => generation === onboardingGeneration && onboardingHere(3), (view) => {
    const found = view?.candidates?.find((c) => c.id === provider), mine = onboardingWriterCandidate(provider);
    if (found && mine) Object.assign(mine, found);
    drawOnboardingWriting();
  });
}
async function loadOnboardingOllama(generation) {
  clearTimeout(onboardingOllamaTimer);
  const here = () => onboardingOllamaHere(generation);
  try {
    const view = await api('/api/writing/ollama');
    if (!here()) return;
    showOnboardingOllama(view, generation);
  } catch (e) {
    if (!here()) return;
    onboardingOllamaTimer = setTimeout(() => loadOnboardingOllama(generation), 5000);
  }
}
function showOnboardingOllama(view, generation) {
  clearTimeout(onboardingOllamaTimer);
  const pulling = view.pull?.state === 'pulling';
  if (onboardingOllama?.pull?.state === 'pulling' && view.pull?.state === 'done') onboardingOllamaPick = view.pull.model;
  state.ollama = onboardingOllama = view;
  Object.assign(onboardingWriterCandidate('ollama'), { installed: view.installed, running: view.running, models: view.pulled || [] });
  drawOnboardingWriting();
  if (pulling || !onboardingWriterReady(onboardingWriterCandidate('ollama'))) onboardingOllamaTimer = setTimeout(() => loadOnboardingOllama(generation), pulling ? 1000 : 5000);
}
function onboardingOllamaHere(generation) { return generation === onboardingGeneration && onboardingHere(3) && onboardingWriter === 'ollama'; }
function pickOnboardingOllama(model) {
  onboardingOllamaPick = model;
  drawOnboardingWriting();
}
async function onboardingUse() {
  const c = onboardingWriterCandidate(onboardingWriter);
  if (!onboardingWriterUsable(c) || onboardingBusy || !onboardingHere(3)) return;
  onboardingBusy = true;
  drawOnboardingWriting();
  try {
    await api('/api/settings', { method: 'POST', body: { set: 'provider', task: 'writing', value: c.id, ...(c.id === 'ollama' && onboardingOllamaPick ? { model: onboardingOllamaPick } : {}) } });
  } finally {
    onboardingBusy = false;
    drawOnboardingWriting();
  }
  if (onboardingHere(3)) await finishOnboarding({ dataset: { step: '3' } });
}
