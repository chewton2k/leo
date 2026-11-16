const WRITING_PRIVACY = {
  claude_code: 'Notes and transcripts you send go to Anthropic through Claude Code.',
  codex: 'Notes and transcripts you send go to OpenAI through Codex.',
  ollama: 'Ollama runs on this computer; nothing leaves it.',
};
const TASK_TITLE = { writing: 'AI for writing', speech: 'AI for speech' };
const TASK_USE = {
  writing: 'Turns recordings into notes, powers Felix and note connections.',
  speech: 'Turns what was said into text while you record.',
};

async function showSettings(section) {
  section = settingsSection(section);
  // Trash is a page of notes rather than a setting, so a bare #/settings never reopens it.
  if (section === 'trash') return showTrash();
  try { localStorage.setItem('leo-settings-section', section); } catch (e) {}
  if (section === 'keeping') return showKeeping();
  const mine = ++seq;
  state = { view: 'settings', dir: '', section };
  scheduleAppStatus(appPollGeneration);
  chrome({});
  $('#crumbs').innerHTML = settingsCrumbs(section);
  document.title = 'Settings · leo';
  refreshPanel(app, settingsFrame(section, skeleton(3, 'settings')));
  const [page, calendar, speech, dictation, margin] = await Promise.all([api('/api/settings'), api('/api/calendar').catch(() => null), api('/api/speech').catch(() => null), api('/api/dictation').catch(() => null), section === 'felix' ? api('/api/margin').catch(() => null) : null]);
  if (mine !== seq) return;
  state.margin = margin;
  state.calendar = calendar;
  state.speech = speech;
  state.dictation = dictation;
  drawSettings(page);
  if (section === 'health') loadHealth().catch(fail);
  startSpeechPolling(mine);
  if (dictationSection(section)) pollDictation(mine, ++dictationGeneration);
}

const CALENDAR_PROVIDERS = {
  google: ['https://calendar.google.com/calendar/r/settings', 'Google Calendar settings', 'Under <b>Settings for my calendars</b>, pick the calendar.', 'In <b>Integrate calendar</b>, copy <b>Secret address in iCal format</b>.'],
  outlook: ['https://outlook.live.com/calendar/0/options/calendar/SharedCalendars', 'Outlook calendar settings', 'Under <b>Shared calendars</b> and <b>Publish a calendar</b>, pick the calendar and choose <b>Publish</b>.', 'Copy the <b>ICS</b> link.'],
  apple: ['https://www.icloud.com/calendar', 'iCloud Calendar', 'Share the calendar as a <b>Public Calendar</b>.', 'Copy the link, which starts with <b>webcal://</b>.'],
};

function calendarSteps(provider) {
  const [url, name, second, third] = CALENDAR_PROVIDERS[provider] || CALENDAR_PROVIDERS.google;
  return `<ol class="cal-steps">
        <li>Open <a href="${url}" target="_blank" rel="noopener noreferrer">${name}</a>.</li>
        <li>${second}</li>
        <li>${third}</li>
      </ol>`;
}

function calendarAddressError(link, provider) {
  if (!/^(https|webcal):\/\//i.test(link)) return 'Paste the calendar address that starts with https:// or webcal://.';
  const share = link.includes('calendar.google.com/calendar/u/') || (link.includes('?cid=') && !link.includes('.ics'));
  return provider === 'google' && share ? 'That is the share link. Paste the secret address that ends in .ics.' : '';
}

function showCalendarError(message) {
  const box = $('#calendar-link'), note = $('#calendar-error');
  if (box) box.setAttribute('aria-invalid', String(Boolean(message)));
  if (note) note.textContent = message;
}

document.addEventListener('change', (e) => {
  const field = e.target.closest?.('[data-cal-provider]');
  const steps = field && $('.cal-steps');
  if (!steps) return;
  state.calendarProvider = field.value;
  steps.outerHTML = calendarSteps(field.value);
  showCalendarError('');
});
document.addEventListener('input', (e) => {
  if (e.target.id === 'calendar-link') showCalendarError('');
});
document.addEventListener('change', async (e) => {
  const field = e.target;
  if (!field.matches?.('[data-cal-offer]')) return;
  try {
    state.calendar = await api('/api/calendar', { method: 'PATCH', body: { offer: field.checked } });
  } catch (error) {
    field.checked = !field.checked;
    fail(error);
  }
});

function calendarCard(cal) {
  if (!cal) return '';
  const linked = cal.calendars || [];
  const count = linked.length;
  const chip = count ? `<span class="set-status ok">${count === 1 ? 'Connected' : `${count} calendars`}</span>` : '<span class="set-status missing">Not connected</span>';
  const rows = linked
    .map((c) => `<div class="set-row cal-row">
      <span class="cal-icon">${ICON.calendar}</span>
      <span class="grow"><span class="feed-name">${esc(c.name)}</span>${c.problem ? `<span class="set-note warn">${esc(c.problem)}</span>` : `<span class="feed-sub">Added ${esc(rel(c.added_at))}</span>`}</span>
      <button class="btn plain sm" data-action="calendar-remove" data-id="${esc(c.id)}" data-name="${esc(c.name)}">Remove</button>
    </div>`)
    .join('');
  const open = !count || state.calendarAdding;
  const provider = state.calendarProvider || 'google';
  const form = `<div class="set-row"><span class="set-label">Connect a calendar</span>${segmented({ label: 'Connect a calendar', field: 'data-cal-provider', current: provider, choices: [['google', 'Google'], ['outlook', 'Outlook'], ['apple', 'Apple']] })}</div>
      ${calendarSteps(provider)}
      <div class="cal-add"><input id="calendar-link" class="set-input" type="text" inputmode="url" autocomplete="off" spellcheck="false" placeholder="Paste the address here" aria-describedby="calendar-error"><button class="btn primary sm" data-action="calendar-add">Connect</button></div>
      <p class="set-note warn" id="calendar-error" role="alert"></p>`;
  const adder = open
    ? `<div class="cal-form">${count ? '<div class="cal-form-head"><span>Add another calendar</span><button class="btn plain sm" data-action="calendar-cancel">Cancel</button></div>' : ''}${form}</div>`
    : `<button class="set-advanced cal-more" data-action="calendar-more">${ICON.plus}<span class="grow">Add another calendar</span>${ICON.chevron}</button>`;
  return `<section class="set-card cal-card" data-find="calendar-connect">
    <header><h3>Calendar</h3>${chip}</header>
    <p class="hint">Recordings are named after the class or meeting you are in, and the AI gets its description and who was invited. leo only reads your calendar.</p>
    ${rows}
    ${count ? `<label class="set-row" data-find="calendar-offer"><span class="grow">Offer to record when an event starts<span class="set-desc">Ask whether to record when a connected calendar event begins.</span></span>${settingsSwitch('data-cal-offer', cal.offer !== false)}</label>` : ''}
    ${adder}
  </section>`;
}

async function addCalendar() {
  const box = $('#calendar-link');
  if (!box || !box.value.trim()) throw Object.assign(new Error('Paste your calendar’s secret address first.'), { shown: true });
  const bad = calendarAddressError(box.value.trim(), state.calendarProvider || 'google');
  showCalendarError(bad);
  if (bad) return;
  const button = $('[data-action="calendar-add"]');
  if (button) {
    button.disabled = true;
    button.textContent = 'Checking…';
  }
  const mine = seq;
  try {
    const calendar = await api('/api/calendar', { method: 'POST', body: { link: box.value.trim() } });
    if (mine !== seq || state.view !== 'settings') return;
    state.calendar = calendar;
    state.calendarAdding = false;
    drawSettings(state.settings);
    const added = calendar.calendars[calendar.calendars.length - 1];
    toast(`Connected “${added ? added.name : 'your calendar'}”.`);
  } catch (e) {
    if (button && button.isConnected) {
      button.disabled = false;
      button.textContent = 'Connect';
    }
    throw e;
  }
}

async function removeCalendar(el) {
  const mine = seq;
  const calendar = await api(`/api/calendar/${enc(el.dataset.id)}`, { method: 'DELETE' });
  if (mine !== seq || state.view !== 'settings') return;
  state.calendar = calendar;
  drawSettings(state.settings);
  toast(`Removed “${el.dataset.name}”.`);
}

function settingsOption(value, label, current) {
  return `<option value="${esc(value)}"${value === current ? ' selected' : ''}>${esc(label)}</option>`;
}

const effortName = (e) => (e === 'xhigh' ? 'Extra high' : e[0].toUpperCase() + e.slice(1));

function modelsByRelease(models) {
  const version = (id) => {
    const found = /(\d+)(?:[.-](\d+))?/.exec(id);
    return found ? [Number(found[1]), Number(found[2] || 0)] : [-1, 0];
  };
  return models
    .map((m, power) => ({ m, power, v: version(m.id) }))
    .sort((a, b) => b.v[0] - a.v[0] || b.v[1] - a.v[1] || b.power - a.power)
    .map((entry) => entry.m);
}

function modelPicker(t) {
  const known = t.models.some((m) => m.id === t.model);
  const sorted = t.provider === 'codex' || t.provider === 'claude_code' ? modelsByRelease(t.models) : t.models;
  const list = (known || !t.model ? [] : [{ id: t.model, price: 'not in the list' }]).concat(sorted);
  const efforts = Array.isArray(t.efforts) ? t.efforts : [];
  const effort = efforts.includes(t.effort) ? t.effort : 'medium';
  const shown = `${esc(t.model || 'Default model')}${efforts.length ? `<span class="picker-effort"> · ${esc(`${effortName(effort).toLowerCase()} effort`)}</span>` : ''}`;
  const rows = list
    .map((m) => {
      const on = m.id === t.model;
      return `<button type="button" class="pick-model${on ? ' on' : ''}" role="menuitemradio" aria-checked="${on}" data-action="set-model" data-task="${t.task}" data-model="${esc(m.id)}"><span class="pick-text"><span class="pick-name">${esc(m.id)}</span><span class="pick-price">${esc(m.price)}</span></span>${on ? ICON.check : ''}</button>`;
    })
    .join('');
  const chips = efforts.length
    ? `<div class="pick-effort"><div class="pick-head">Effort</div><div class="pick-chips">${efforts.map((e) => `<button type="button" class="pick-chip${effort === e ? ' on' : ''}" aria-pressed="${effort === e}" data-action="set-effort" data-effort="${e}">${effortName(e)}</button>`).join('')}</div><p class="pick-hint">More effort means the model thinks longer: better answers, but slower and more of your plan or credit.</p></div>`
    : '';
  return `<div class="set-row" data-find="writing-model"><span class="set-label">Model</span><div class="picker"><button type="button" class="picker-button" data-action="toggle-picker" aria-haspopup="menu" aria-expanded="false" data-task="${t.task}"><span class="picker-text">${shown}</span>${ICON.chevron}</button><div class="picker-menu" role="menu" hidden><div class="pick-head">Model</div><div class="pick-models">${rows}</div>${chips}</div></div></div>`;
}

function closePickers(except) {
  for (const menu of app.querySelectorAll('.picker-menu')) {
    if (menu === except) continue;
    fold(menu);
    const button = menu.parentElement.querySelector('.picker-button');
    if (button) button.setAttribute('aria-expanded', 'false');
  }
}

function togglePicker(el) {
  const menu = el.parentElement.querySelector('.picker-menu');
  closePickers(menu);
  const opening = menu.hidden || Boolean(menu.dataset.folding);
  if (opening) {
    unfold(menu, el);
    (menu.querySelector('.pick-model.on') || menu.querySelector('.pick-model'))?.focus();
  } else fold(menu);
  el.setAttribute('aria-expanded', String(opening));
}

document.addEventListener('click', (e) => {
  if (state.view === 'settings' && !e.target.closest('.picker')) closePickers();
});
document.addEventListener('keydown', (e) => {
  if (e.key === 'Escape' && state.view === 'settings' && app.querySelector('.picker-menu:not([hidden])')) {
    e.stopPropagation();
    const open = app.querySelector('.picker-menu:not([hidden])');
    const owner = open.parentElement.querySelector('.picker-button');
    closePickers();
    owner?.focus();
    return;
  }
  if (state.view !== 'settings') return;
  const row = e.target.closest?.('.picker-menu:not([hidden]) .pick-model');
  if (row) {
    const menu = row.closest('.picker-menu');
    const to = listKeyTarget([...menu.querySelectorAll('.pick-model')], row, e, (el) => el.querySelector('.pick-name').textContent);
    if (to) { e.preventDefault(); to.focus(); }
    return;
  }
  const button = e.target.closest?.('.picker-button');
  if (button && button.getAttribute('aria-expanded') !== 'true' && (e.key === 'ArrowDown' || e.key === 'ArrowUp')) {
    e.preventDefault();
    togglePicker(button);
  }
}, true);

function providerCard(c) {
  const cards = {
    ollama: ['Ollama', ON_MAC ? 'On this Mac' : 'On this computer', ICON.logoOllama],
    codex: ['Codex', 'ChatGPT plan', ICON.logoOpenai],
    claude_code: ['Claude Code', 'Claude plan', ICON.logoClaude],
  };
  const [title, about, icon] = cards[c.id] || [c.label, '', ''];
  return [c.id, title, about, '', icon];
}

const SIGN_IN_POLL_MS = 3000, SIGN_IN_WAIT_MS = 120000;
const SIGN_IN_LATE = 'Still waiting. Finish signing in, then press Check again.';
const INSTALL_URL = { claude_code: 'https://claude.com/claude-code', codex: 'https://github.com/openai/codex' };
let signInWait = null;
function signInLate(id) { return signInWait?.provider === id && signInWait.late; }
function signInControl(c, action) {
  if (!INSTALL_URL[c.id]) return '';
  if (!c.installed) return `<a class="btn plain sm" href="${INSTALL_URL[c.id]}" target="_blank" rel="noopener noreferrer">Install ${providerCard(c)[1]}</a>`;
  if (signInWait?.provider === c.id && !signInWait.late) return '<button class="btn plain sm" disabled>Waiting for sign-in…</button>';
  return `<button class="btn plain sm" data-action="${action}" data-provider="${c.id}"${signInLate(c.id) ? ' data-check="1">Check again' : '>Sign in'}</button>`;
}
async function signIn(el, here, draw) {
  const provider = el.dataset.provider, check = el.dataset.check === '1';
  if (!INSTALL_URL[provider]) return;
  clearTimeout(signInWait?.timer);
  const wait = signInWait = { provider, late: false, timer: 0 };
  const until = Date.now() + (check ? 0 : SIGN_IN_WAIT_MS);
  const gone = () => { if (signInWait === wait) signInWait = null; };
  const tick = async () => {
    if (signInWait !== wait) return;
    if (!here()) return gone();
    let view = null;
    try { view = await api('/api/writing?probe=signin'); } catch (e) {}
    if (signInWait !== wait) return;
    if (!here()) return gone();
    if (view?.candidates?.find((c) => c.id === provider)?.signed_in === true) gone();
    else if (Date.now() >= until) wait.late = true;
    else { wait.timer = setTimeout(tick, SIGN_IN_POLL_MS); return; }
    draw(view);
  };
  if (!check) {
    try { await appChange({ action: 'sign_in', provider }); } catch (e) { gone(); if (here()) draw(null); throw e; }
    if (signInWait !== wait) return;
    if (!here()) return gone();
  }
  draw(null);
  wait.timer = setTimeout(tick, check ? 0 : SIGN_IN_POLL_MS);
}
function writingSignIn(el) {
  const mine = seq, provider = el.dataset.provider;
  return signIn(el, () => mine === seq && state.view === 'settings' && state.section === 'writing' && !!state.settings, (view) => {
    const found = view?.candidates?.find((c) => c.id === provider);
    const plan = state.settings.plans?.find((p) => p.provider === provider);
    if (found && plan) Object.assign(plan, { installed: found.installed, signed_in: found.signed_in });
    if (view?.writing) state.settings.writing = view.writing;
    drawSettings(state.settings);
  });
}

function taskCard(t) {
  const plan = state.settings?.plans?.find(plan => plan.provider === t.provider);
  const status = plan && plan.installed && plan.signed_in !== true
    ? `<span class="set-status missing">${plan.signed_in === false ? 'Sign in needed' : 'Sign-in unknown'}</span>`
    : state.settings?.writing?.state === 'ready' ? '<span class="set-status ok">Ready</span>' : '<span class="set-status missing">Not set up</span>';
  const providers = t.choices.map((c) => settingsOption(c.id, c.label, t.provider)).join('') + (t.custom ? settingsOption(t.provider, `${t.provider} (from config.toml)`, t.provider) : '');
  let model = '';
  if (t.fixed_model) {
    model = `<div class="set-row" data-find="writing-model"><span class="set-label">Model</span><span class="set-value">${esc(t.model || 'Built in')} <span class="hint">free, runs on your computer</span></span></div>`;
  } else if (t.models.length) {
    model = modelPicker(t);
  }
  let key = '';
  if (t.key) {
    const has = t.key.stored;
    const form = `<div class="set-key-form"><input type="password" autocomplete="off" spellcheck="false"${has ? ` class="has-key" placeholder="${esc(t.key.hint || '••••••••')}" aria-label="${esc(t.key.name)} key is stored; type a new one to replace it"` : ` placeholder="Paste your ${esc(t.key.name)} API key"`} data-key-input="${esc(t.key.account)}"><button class="btn primary sm" data-action="set-key" data-account="${esc(t.key.account)}">${has ? 'Replace key' : 'Save key'}</button></div>`;
    key = `<div class="set-row column" data-find="writing-key"><div class="set-line"><span class="set-label">${esc(t.key.name)} key</span><span class="set-value">${has ? '' : '<span class="set-status missing">Not added</span>'}${has ? `<button class="btn plain sm" data-action="remove-key" data-account="${esc(t.key.account)}" data-name="${esc(t.key.name)}">Remove</button>` : ''}</span></div>${form}${t.key.ignored ? `<p class="hint">leo does not read $${esc(t.key.ignored)}; add the key here instead.</p>` : ''}</div>`;
  }
  const signin = t.signin ? `<p class="set-note${t.signin.installed ? '' : ' warn'}">${esc(t.signin.text).replace(/`([^`]+)`/g, '<code>$1</code>')}</p>` : '';
  const signInRow = plan && plan.signed_in !== true && INSTALL_URL[plan.provider] ? `<div class="set-row"><span class="grow hint">${signInLate(plan.provider) ? SIGN_IN_LATE : ''}</span>${signInControl({ id: plan.provider, installed: plan.installed }, 'writing-sign-in')}</div>` : '';
  const usage = t.usage && !plan ? `<p class="set-note">Plan used: ${esc(t.usage)}</p>` : '';
  const ollama = t.task === 'writing' && t.provider === 'ollama';
  const note = t.note && !(ollama && state.ollama && !state.ollama.installed) ? `<p class="set-note warn">${esc(t.note)}</p>` : '';
  const privacy = t.task === 'writing' && WRITING_PRIVACY[t.provider] ? `<p class="set-note">${WRITING_PRIVACY[t.provider]}</p>` : '';
  return `<section class="set-card" data-task-card="${t.task}" data-find="writing-provider">
    <header><h3>${TASK_TITLE[t.task]}</h3>${status}</header>
    <p class="hint">${TASK_USE[t.task]}</p>
    ${t.custom || t.choices.length > 4
      ? `<label class="set-row"><span class="set-label">Provider</span><span class="select-control"><select data-set="provider" data-task="${t.task}">${providers}</select>${ICON.chevron}</span></label>`
      : `<div class="set-row column"><span class="set-label">Provider</span>${choiceCards({ label: 'Provider', choices: t.choices.map((c) => providerCard(c)), current: t.provider, field: `data-set="provider" data-task="${t.task}"`, className: 'providers' })}</div>`}
    ${privacy}${model}${key}${signin}${signInRow}${usage}${note}${ollama ? `<div id="ollama-setup">${ollamaSetup(state.ollama, t.model)}</div>` : ''}
    <div class="set-test" data-find="writing-test"><button class="btn plain sm" data-action="test-ai" data-task="${t.task}">Test connection</button><span class="set-result" id="test-${t.task}"></span></div>
  </section>`;
}

function ollamaGB(bytes, unit = 1e9) {
  const n = (Number(bytes) || 0) / unit;
  return `${n >= 10 ? Math.round(n) : Math.round(n * 10) / 10} GB`;
}

function ollamaSetup(view, current, picked = ollamaSelected) {
  if (!view || (view.installed && !view.running)) return '';
  const where = ON_MAC ? 'this Mac' : 'this computer';
  if (!view.installed) return `<div class="set-row"><span class="grow">Ollama runs AI models on ${where}. It isn’t installed yet.</span><a class="btn primary sm" href="https://ollama.com/download" target="_blank" rel="noopener noreferrer">Download Ollama</a></div><p class="set-note">Once it’s installed, leo finds it by itself and lists models that fit ${where}. No Terminal needed.</p>`;
  const pull = view.pull?.state === 'pulling' ? view.pull : null;
  const percent = pull ? Math.min(100, Math.floor((pull.completed || 0) * 100 / Math.max(1, pull.total || 0))) : 0;
  const facts = [view.memory_bytes && `${ollamaGB(view.memory_bytes, 1073741824)} of memory`, view.free_disk_bytes && `${ollamaGB(view.free_disk_bytes)} free`].filter(Boolean);
  const summary = facts.length ? `<p class="hint">${ON_MAC ? 'This Mac' : 'This computer'} has ${facts.join(' and ')}.</p>` : '';
  const cards = (view.models || []).map((m) => {
    const here = pull?.model === m.name, on = here || m.name === picked;
    const fit = m.recommended ? 'Recommended' : `${m.fits ? 'Fits' : 'Too big for'} ${where}`;
    const status = here ? `<span class="chip accent">Downloading ${percent}%</span>` : m.name === current ? '<span class="chip accent">In use</span>' : m.pulled ? '<span class="chip">Downloaded</span>' : '';
    return `<button type="button" class="onboarding-model${on ? ' on' : ''}" data-action="ollama-pick" data-model="${esc(m.name)}" aria-pressed="${on}"${m.fits ? '' : ' disabled'}><span class="onboarding-model-head"><b>${esc(m.name)}</b><span class="chip">${fit}</span><span class="onboarding-model-state">${status}</span></span><span class="hint">${esc(m.about)}</span><span class="hint">${ollamaGB(m.bytes)} · needs ${esc(m.memory_gb)} GB of memory</span></button>`;
  }).join('');
  const progress = pull ? `<div class="onboarding-model-action"><div class="grow onboarding-download"><span class="hint">Downloading ${esc(pull.model)} · ${pull.total ? `${ollamaGB(pull.completed)} of ${ollamaGB(pull.total)}` : 'Starting…'}</span><progress value="${percent}" max="100" aria-label="Downloading ${esc(pull.model)}"></progress></div><button class="btn plain" data-action="ollama-cancel">Cancel</button></div>` : '';
  const choice = !pull && (view.models || []).find((m) => m.name === picked && m.fits && (!m.pulled || m.name !== current));
  const choose = choice ? `<div class="onboarding-model-action"><span class="grow hint">${esc(choice.name)} ${choice.pulled ? 'is ready on this computer.' : 'downloads once, then works offline.'}</span><button class="btn primary" data-action="ollama-pull" data-model="${esc(choice.name)}">${choice.pulled ? 'Use' : `Download · ${ollamaGB(choice.bytes)}`}</button></div>` : '';
  const failed = view.pull?.state === 'failed' ? `<p class="set-note warn">${esc(view.pull.error)}</p>` : '';
  return `${summary}<div class="onboarding-model-list" role="group" aria-label="Ollama models">${cards}</div>${progress}${choose}${failed}`;
}

let ollamaTimer = 0, ollamaGeneration = 0;
function writingModel() {
  return state.settings?.tasks?.find((t) => t.task === 'writing')?.model;
}
async function loadOllama(mine) {
  clearTimeout(ollamaTimer);
  const generation = ++ollamaGeneration;
  const here = () => generation === ollamaGeneration && mine === seq && state.view === 'settings' && $('#ollama-setup');
  if (!here()) return;
  if (appVisible()) {
    try {
      const before = state.ollama;
      const view = await api('/api/writing/ollama');
      if (!here()) return;
      state.ollama = view;
      if (before?.pull?.state === 'pulling' && view.pull?.state === 'done') return await changeSetting({ set: 'model', task: 'writing', value: view.pull.model });
      if (before && (before.installed !== view.installed || before.running !== view.running)) {
        const page = await api('/api/settings');
        if (here()) drawSettings(page);
        return;
      }
      refreshPanel($('#ollama-setup'), ollamaSetup(view, writingModel()));
    } catch (e) {
      if (!here()) return;
    }
  }
  const wait = state.ollama?.pull?.state === 'pulling' ? 1000 : state.ollama?.running ? 0 : 5000;
  if (wait) ollamaTimer = setTimeout(() => loadOllama(mine), wait);
}
async function ollamaChange(change) {
  const mine = seq, setup = onboardingHere(3) ? onboardingGeneration : null;
  const view = await api('/api/writing/ollama', { method: 'POST', body: change });
  if (setup !== null) return onboardingOllamaHere(setup) && showOnboardingOllama(view, setup);
  if (mine !== seq || state.view !== 'settings') return;
  state.ollama = view;
  refreshPanel($('#ollama-setup'), ollamaSetup(view, writingModel()));
  loadOllama(mine);
}
let ollamaSelected = '';
function ollamaPick(el) {
  ollamaSelected = el.dataset.model;
  if (onboardingHere(3)) return drawOnboardingWriting();
  refreshPanel($('#ollama-setup'), ollamaSetup(state.ollama, writingModel()));
}
function ollamaPull(el) {
  const model = state.ollama?.models?.find((m) => m.name === el.dataset.model);
  if (!model || state.ollama.pull?.state === 'pulling') return;
  if (model.pulled) return onboardingHere(3) ? pickOnboardingOllama(model.name) : changeSetting({ set: 'model', task: 'writing', value: model.name });
  return ollamaChange({ action: 'pull', model: model.name });
}

const PLAN_STALE_MS = 5 * 60 * 1000;

function planLimits(plan) {
  const limits = plan.limits;
  const now = Date.now();
  return [['five_hour', 'Session', '5 hour'], ['seven_day', 'Weekly', '7 day']].map(([key, name, period]) => {
    const window = limits?.[key];
    const used = window?.used;
    const valid = Number.isFinite(used);
    const reset = Number.isFinite(window?.resets_at) ? new Date(window.resets_at * 1000) : null;
    const validReset = reset && Number.isFinite(reset.getTime());
    const expired = validReset && reset.getTime() <= now;
    const available = valid && !expired;
    const percent = available ? Math.max(0, Math.min(100, 100 - Math.round(used * 100))) : null;
    const missing = expired ? 'Reset' : 'Not read yet';
    const when = !available ? 'Refresh to check' : validReset ? `Resets in ${duration(reset.getTime() - now)}` : 'Reset time unavailable';
    return `<div class="plan-window"><div class="plan-metric"><span>${name}</span><strong${available ? '' : ' class="plan-none"'}>${available ? `${percent}% <small>left</small>` : missing}</strong></div><div class="plan-track"><div class="plan-meter${available ? '' : ' unavailable'}" role="meter" aria-label="${esc(plan.name)} ${period} remaining" aria-valuemin="0" aria-valuemax="100"${available ? ` aria-valuenow="${percent}"` : ` aria-valuetext="${missing}"`}>${available ? `<i style="width:${percent}%"></i>` : ''}</div><span class="hint">${esc(when)}</span></div></div>`;
  }).join('');
}

function planUpdated(plan) {
  const limits = plan.limits;
  const now = Date.now();
  const seenAt = typeof limits?.seen_at === 'string' && limits.seen_at ? limits.seen_at : null;
  const seen = seenAt ? new Date(seenAt).getTime() : NaN;
  const stale = Number.isFinite(seen) && now - seen >= PLAN_STALE_MS;
  return Number.isFinite(seen) ? `${stale ? 'Saved · ' : ''}Updated ${rel(seenAt)}` : '';
}

function duration(ms) {
  const minutes = Math.max(1, Math.floor(ms / 60000));
  const days = Math.floor(minutes / 1440), hours = Math.floor((minutes % 1440) / 60), rest = minutes % 60;
  return days ? `${days}d ${hours}h` : hours ? `${hours}h ${rest}m` : `${rest}m`;
}

function codingPlans(page) {
  if (!page.plans?.length) return '';
  return `<div class="coding-plans">${page.plans.map(plan => {
    const connected = plan.installed && plan.signed_in === true;
    const status = !plan.installed ? 'Not available' : connected ? 'Signed in' : plan.signed_in === false ? 'Signed out' : 'Sign-in unknown';
    const updated = planUpdated(plan);
    return `<section class="coding-plan" data-provider="${esc(plan.provider)}" aria-label="${esc(plan.name)} account"><header><div class="plan-title"><h4>${esc(plan.name)}</h4>${updated ? `<span class="hint plan-updated">${esc(updated)}</span>` : ''}</div><span class="set-status${connected ? ' ok' : ' missing'}">${status}</span></header>${planLimits(plan)}${!connected ? `<p class="hint">${plan.installed ? `Check sign-in with ${esc(plan.signin)}.` : 'Install or update this coding app, then refresh Usage.'}</p>` : ''}</section>`;
  }).join('')}</div>`;
}

const APPEARANCE_DEFAULTS = { theme: 'system', felix: 'cute', width: 'wide', size: 'medium', letters: 'own' };
const THEME_LABELS = { system: 'Match device', light: 'Light', dark: 'Dark' };

const THEME_SURFACES = {
  light: { ground: '#f0eee8', page: '#faf9f6', head: '#1c1c1a', line: '#d9d8d3', side: '#e3e1db' },
  dark: { ground: '#181715', page: '#22211f', head: '#e9e7e2', line: '#3a3835', side: '#2b2a27' },
};
function themeScene(look) {
  const c = THEME_SURFACES[look];
  return `<rect width="120" height="52" fill="${c.ground}"/><rect x="7" y="10" width="15" height="3" rx="1.5" fill="${c.side}"/><rect x="7" y="17" width="11" height="3" rx="1.5" fill="${c.side}"/><rect x="7" y="24" width="13" height="3" rx="1.5" fill="${c.side}"/><rect x="29" y="5" width="96" height="52" rx="5" fill="${c.page}"/><rect x="40" y="14" width="30" height="4" rx="2" fill="${c.head}"/><rect x="40" y="24" width="64" height="3" rx="1.5" fill="${c.line}"/><rect x="40" y="31" width="56" height="3" rx="1.5" fill="${c.line}"/><rect x="40" y="38" width="42" height="3" rx="1.5" fill="${c.line}"/>`;
}
const THEME_DIAGRAMS = {
  system: `<svg viewBox="0 0 120 52">${themeScene('light')}<svg x="60" width="60" height="52" viewBox="60 0 60 52">${themeScene('dark')}</svg></svg>`,
  light: `<svg viewBox="0 0 120 52">${themeScene('light')}</svg>`,
  dark: `<svg viewBox="0 0 120 52">${themeScene('dark')}</svg>`,
};
const TEXT_SIZE_LABELS = { small: 'Small', medium: 'Medium', large: 'Large' };
const LETTERING_LABELS = { own: "Felix's own", notes: 'Like your notes' };
const LETTERING_SAMPLE = 'Found it, in your Tuesday note.';
function themeCard() {
  const now = window.leoTheme.choice();
  const felixNow = window.leoTheme.felix();
  const sizeNow = window.leoTheme.textSize();
  const lettersNow = window.leoTheme.felixLetters();
  return `<section class="set-card appearance">
      <header><h3>Appearance</h3></header>
      <div class="set-row column" data-find="theme"><span class="set-label">Theme${settingReset('Theme', 'theme', now !== APPEARANCE_DEFAULTS.theme)}</span>${choiceCards({ label: 'Theme', action: 'theme-pick', key: 'theme', current: now, choices: window.leoTheme.CHOICES.map((choice) => [choice, THEME_LABELS[choice], '', THEME_DIAGRAMS[choice]]) })}</div>
      <div class="set-row column" data-find="felix-look"><span class="set-label">Felix${settingReset('Felix', 'felix', felixNow !== APPEARANCE_DEFAULTS.felix)}</span>${choiceCards({ label: 'Felix', action: 'felix-pick', key: 'felix', current: felixNow, choices: [['cute', 'Cute'], ['demon', 'Demon']].map(([kind, label]) => [kind, label, '', `<svg viewBox="0 0 240 52">${felix.felix(40, `felix-preview ${kind}-look`).replace('<svg ', '<svg x="100" y="10" ')}</svg>`]) })}</div>
      ${widthRow()}
      <div class="set-row column" data-find="text-size"><span class="set-label">Text size${settingReset('Text size', 'size', sizeNow !== APPEARANCE_DEFAULTS.size)}</span>${choiceCards({ label: 'Text size', action: 'text-size-pick', key: 'size', current: sizeNow, choices: window.leoTheme.TEXT_SIZES.map((size) => [size, TEXT_SIZE_LABELS[size], '', `<span class="read-sample size" data-text-size="${size}">Notes</span>`]) })}</div>
      <div class="set-row column" data-find="felix-lettering"><span class="set-label">Felix's lettering${settingReset("Felix's lettering", 'letters', lettersNow !== APPEARANCE_DEFAULTS.letters)}</span>${choiceCards({ label: "Felix's lettering", action: 'letters-pick', key: 'letters', current: lettersNow, choices: window.leoTheme.LETTERINGS.map((letters) => [letters, LETTERING_LABELS[letters], '', `<span class="read-sample letters" data-letters="${letters}">${LETTERING_SAMPLE}</span>`]) })}</div>
    </section>`;
}

function syncAppearanceResets() {
  const now = { theme: window.leoTheme.choice(), felix: window.leoTheme.felix(), size: window.leoTheme.textSize(), letters: window.leoTheme.felixLetters() };
  for (const button of document.querySelectorAll('[data-action="setting-reset"]')) button.toggleAttribute('data-on', now[button.dataset.reset] !== APPEARANCE_DEFAULTS[button.dataset.reset]);
}
function resetSetting(el) {
  const key = el.dataset.reset;
  if (key === 'theme') pickTheme({ dataset: { theme: APPEARANCE_DEFAULTS.theme } });
  else if (key === 'felix') pickFelix({ dataset: { felix: APPEARANCE_DEFAULTS.felix } });
  else if (key === 'size') pickTextSize({ dataset: { size: APPEARANCE_DEFAULTS.size } });
  else if (key === 'letters') pickLetters({ dataset: { letters: APPEARANCE_DEFAULTS.letters } });
}

function pickTheme(el) {
  window.leoTheme.set(el.dataset.theme);
  for (const button of document.querySelectorAll('[data-action="theme-pick"]')) {
    const on = button.dataset.theme === window.leoTheme.choice();
    button.classList.toggle('on', on);
    button.setAttribute('aria-pressed', String(on));
  }
  // Callers may pass just { dataset }, so the highlight follows the page's own buttons.
  const box = document.querySelector('[data-action="theme-pick"]')?.closest('.choice-cards');
  if (box) syncChoiceCards(box);
  syncAppearanceResets();
}

const WIDTH_LABELS = { comfortable: 'Comfortable', wide: 'Wide', full: 'Full width' };

function widthRow() {
  const dragged = window.leoTheme.pageWidth(), width = window.leoTheme.noteWidth();
  const now = width === 'full' ? 'Full width.' : dragged ? `${dragged} px wide.` : width === APPEARANCE_DEFAULTS.width ? 'The default.' : `${WIDTH_LABELS[width]}.`;
  const changed = width !== APPEARANCE_DEFAULTS.width || Boolean(dragged);
  return `<div class="set-row width-row" data-find="note-width"><span class="grow">Note width<span class="set-desc">${now} Drag either mark at the note's top corners; double-click one to reset.</span></span><button type="button" class="btn sm" data-action="width-reset"${changed ? '' : ' disabled'}>Reset</button></div>`;
}

function resetWidth() {
  window.leoTheme.setNoteWidth(APPEARANCE_DEFAULTS.width);
  const row = document.querySelector('[data-find="note-width"]');
  if (row) row.outerHTML = widthRow();
}

function pickFelix(el) {
  window.leoTheme.setFelix(el.dataset.felix);
  for (const button of document.querySelectorAll('[data-action="felix-pick"]')) {
    const on = button.dataset.felix === window.leoTheme.felix();
    button.classList.toggle('on', on);
    button.setAttribute('aria-pressed', String(on));
  }
  // Callers may pass just { dataset }, so the highlight follows the page's own buttons.
  const box = document.querySelector('[data-action="felix-pick"]')?.closest('.choice-cards');
  if (box) syncChoiceCards(box);
  syncAppearanceResets();
}

function pickReading(action, field, now) {
  for (const button of document.querySelectorAll(`[data-action="${action}"]`)) {
    const on = button.dataset[field] === now;
    button.classList.toggle('on', on);
    button.setAttribute('aria-pressed', String(on));
  }
  const box = document.querySelector(`[data-action="${action}"]`)?.closest('.choice-cards');
  if (box) syncChoiceCards(box);
  syncAppearanceResets();
}
function pickTextSize(el) {
  window.leoTheme.setTextSize(el.dataset.size);
  pickReading('text-size-pick', 'size', window.leoTheme.textSize());
}
function pickLetters(el) {
  window.leoTheme.setFelixLetters(el.dataset.letters);
  pickReading('letters-pick', 'letters', window.leoTheme.felixLetters());
}

let felixActivity = { span: 30, day: null, pin: null, ranks: null, view: null, processes: null, busy: false, roll: false, drawn: false, timer: 0, watching: false, watch: null };
const faBound = new WeakSet();
const FA_HEIGHT = 118;
const FA_RING = 2 * Math.PI * 52;
const FA_NEUTRAL = 'color-mix(in srgb, var(--text) 45%, transparent)';

function faToday(hourly) {
  const now = new Date();
  const day = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, '0')}-${String(now.getDate()).padStart(2, '0')}`;
  return hourly ? `${day} ${String(now.getHours()).padStart(2, '0')}` : day;
}
function faSpanName(span) { return span === 1 ? 'last 24 hours' : `last ${span} days`; }
function faCompact(n) {
  if (n >= 999950) return `${(n / 1e6).toFixed(1)}M`;
  if (n >= 1000) return `${(n / 1e3).toFixed(1)}k`;
  return String(n);
}
function faExact(n) { return n.toLocaleString('en-US'); }
function faSeconds(ms) { return `${(ms / 1000).toFixed(1)} s`; }
function faDate(day, long) {
  const [date, hour] = day.split(' ');
  const [y, m, d] = date.split('-').map(Number);
  if (hour !== undefined) return new Date(y, m - 1, d, Number(hour)).toLocaleString('en-US', long ? { weekday: 'short', hour: 'numeric' } : { hour: 'numeric' });
  return new Date(y, m - 1, d).toLocaleDateString('en-US', long ? { weekday: 'short', month: 'short', day: 'numeric' } : { month: 'short', day: 'numeric' });
}
function faBarName(bar, weekly) { return weekly ? `${faDate(bar.from)} – ${faDate(bar.to)}` : faDate(bar.from, true); }
function faModelName(name) { return name || 'Model not reported'; }
function faShare(value, total) {
  const part = total ? value / total * 100 : 0;
  return part > 0 && part < 0.5 ? '<1%' : `${Math.round(part)}%`;
}
function faColour(rank, base) { return rank < 0 ? null : `var(--chart-${rank + base})`; }
function faBg(colour) { return colour ? `background:${colour}` : ''; }
function faCls(colour) { return colour ? '' : ' class="fa-other"'; }
function faNiceMax(v) {
  if (!(v > 0)) return 1;
  const unit = 10 ** Math.floor(Math.log10(v));
  return [1, 2, 2.5, 5, 10].map((m) => m * unit).find((n) => n >= v);
}
function faFamily(name) {
  if (String(name).includes(':')) return 'local';
  if (/^(gpt|o\d|codex|chatgpt)/i.test(name)) return 'openai';
  return /claude|opus|sonnet|haiku|fable/i.test(name) ? 'claude' : 'local';
}
function faShades(ranks) {
  const used = { openai: 0, claude: 0, local: 0 };
  const shades = {};
  for (const name of ranks.models) {
    const family = faFamily(name);
    if (used[family] < 3) shades[name] = `var(--fam-${family}-${++used[family]})`;
  }
  return shades;
}
function faFold(items, ranks) {
  const shades = faShades(ranks);
  return window.leoChat.usageFold(items, Object.keys(shades)).map((item) => ({ ...item, colour: item.other ? null : shades[item.name] }));
}
function faRanks(page, today) {
  const held = felixActivity.ranks;
  if (held && (held.models.length || held.kinds.length)) return held;
  felixActivity.ranks = window.leoChat.usageRank(page.felix_days, today);
  return felixActivity.ranks;
}
function faKindLabel(purpose) {
  const row = window.leoChat.usageKinds([{ purpose, calls: 1 }])[0];
  return row ? row.label : purpose;
}
function faParts(bar, ranks) {
  const items = bar.by.map(([name, value]) => ({ name, value }));
  return faFold(items, ranks).sort((a, b) => (a.rank < 0) - (b.rank < 0) || a.rank - b.rank);
}

function faReadout(i) {
  const { period, ranks } = felixActivity.view;
  const bar = period.bars[i];
  if (!bar) return '';
  const total = bar.input + bar.output;
  const head = bar.calls ? `${bar.calls === 1 ? '1 request' : `${faExact(bar.calls)} requests`} · ${faCompact(bar.input)} in · ${faCompact(bar.output)} out` : 'No requests';
  const parts = faParts(bar, ranks).map((part) => `<span title="${esc(faModelName(part.name))}"><i${faCls(part.colour)} style="${faBg(part.colour)}"></i><em class="fa-rn">${esc(part.other ? 'Other' : faModelName(part.name))}</em><b>${faShare(part.value, total)}</b></span>`).join('');
  return `<div class="fa-rh"><b>${esc(faBarName(bar, period.length === 90))}</b> · ${head}</div><div class="fa-rm">${parts}</div>`;
}

function faDonut(kind, slices, total, label) {
  let at = 0;
  const colourOf = (item) => (kind === 'm' ? item.colour : faColour(item.rank, 4));
  const arcs = slices.map((slice, i) => {
    const arc = slice.value / total * FA_RING;
    const length = Math.max(0, arc - (arc < 3 ? 0 : 2));
    const ring = `<circle class="fa-slice${felixActivity.pin && felixActivity.pin.d === kind && felixActivity.pin.i === i ? ' on' : ''}" data-fa-d="${kind}" data-fa-i="${i}" cx="60" cy="60" r="52" fill="none" stroke="${colourOf(slice) ? colourOf(slice) : `url(#fa-hatch-${kind})`}" stroke-width="12" stroke-dasharray="${length.toFixed(2)} ${(FA_RING - length).toFixed(2)}" stroke-dashoffset="${(-at).toFixed(2)}" transform="rotate(-90 60 60)"/>`;
    at += arc;
    return ring;
  }).join('');
  const pinned = felixActivity.pin && felixActivity.pin.d === kind ? slices[felixActivity.pin.i] : null;
  const shown = pinned ? { big: faCompact(pinned.value), small: pinned.other ? 'Other' : (kind === 'm' ? faModelName(pinned.name) : faKindLabel(pinned.name)) } : { big: faCompact(total), small: label };
  return `<div class="fa-ring${pinned ? ' hl' : ''}"><svg width="120" height="120" viewBox="0 0 120 120" aria-hidden="true"><defs><pattern id="fa-hatch-${kind}" width="5" height="5" patternUnits="userSpaceOnUse" patternTransform="rotate(45)"><path d="M0 0v5" style="stroke:var(--faint)" stroke-width="2.5"/></pattern></defs><mask id="fa-mask-${kind}"><circle class="fa-sweep" cx="60" cy="60" r="52" fill="none" stroke="#fff" stroke-width="16" stroke-dasharray="${FA_RING.toFixed(2)} ${FA_RING.toFixed(2)}" transform="rotate(-90 60 60)"/></mask><g mask="url(#fa-mask-${kind})">${arcs}</g></svg><div class="fa-centre" data-big="${esc(faCompact(total))}" data-small="${esc(label)}"><b>${esc(shown.big)}</b><span>${esc(shown.small)}</span></div></div>`;
}
function faPanel(kind, title, items, unit, label) {
  const colourOf = (item) => (kind === 'm' ? item.colour : faColour(item.rank, 4));
  const total = items.reduce((sum, item) => sum + item.value, 0);
  const name = (item) => item.other ? 'Other' : (kind === 'm' ? faModelName(item.name) : faKindLabel(item.name));
  let body;
  if (!items.length) body = `<p class="set-note">${kind === 'm' ? 'No token counts reported.' : 'No requests.'}</p>`;
  else if (items.length === 1) body = `<p class="fa-one"><i${faCls(colourOf(items[0]))} style="${faBg(colourOf(items[0]))}"></i><b title="${esc(name(items[0]))}">${esc(name(items[0]))}</b><span>${faCompact(items[0].value)} ${unit}</span></p>`;
  else {
    const rows = items.map((item, i) => {
      const on = felixActivity.pin && felixActivity.pin.d === kind && felixActivity.pin.i === i;
      const calls = kind === 'm' ? `<span class="fa-lg-r">${faCompact(item.calls)} req</span>` : '';
      return `<button type="button" class="fa-lg${kind === 'k' ? ' k' : ''}${on ? ' on' : ''}" data-fa-d="${kind}" data-fa-i="${i}" data-big="${esc(faCompact(item.value))}" data-small="${esc(name(item))}" aria-pressed="${on}"><i${faCls(colourOf(item))} style="${faBg(colourOf(item))}"></i><span class="fa-lg-n" title="${esc(name(item))}">${esc(name(item))}</span><span class="fa-lg-v" title="${faExact(item.value)}">${faCompact(item.value)}</span>${calls}<span class="fa-lg-s">${faShare(item.value, total)}</span></button>`;
    }).join('');
    body = `<div class="fa-dp">${faDonut(kind, items, total, label)}<div class="fa-legend">${rows}</div></div>`;
  }
  return `<div class="fa-panel" data-fa-d="${kind}"><div class="fa-h">${title}</div>${body}</div>`;
}

function faPeriod(period, ranks, span) {
  const t = period.totals;
  const bars = period.bars;
  const weekly = period.length === 90;
  const latest = bars.length - 1;
  const at = felixActivity.day !== null && felixActivity.day <= latest ? felixActivity.day : latest;
  const tallest = bars.reduce((best, bar) => (bar.input + bar.output > best.input + best.output ? bar : best), bars[0] || { input: 0, output: 0, by: [] });
  const top = tallest.input + tallest.output;
  const scale = faNiceMax(top);
  const spare = FA_HEIGHT - 2 * Math.max(0, faParts(tallest, ranks).length - 1);
  const days = bars.map((bar, i) => {
    const segments = faParts(bar, ranks).map((part) => {
      const height = top ? part.value / scale * spare : 0;
      return `<i${faCls(part.colour)} style="height:${height.toFixed(1)}px;${faBg(part.colour)}${height < 3 ? ';margin-top:-2px' : ''}"></i>`;
    }).join('');
    return `<div class="fa-day${i === at ? ' on' : ''}" data-i="${i}"><div class="fa-stack">${segments}</div></div>`;
  }).join('');
  const hourly = period.length === 1;
  const step = period.length === 7 ? 1 : weekly ? 4 : hourly ? 6 : 7;
  const labels = bars.map((bar, i) => [i, bar]).filter(([i]) => i === latest || ((latest - i) % step === 0 && (step === 1 || latest - i >= 3))).map(([i, bar]) => `<span style="left:${((i + 0.5) / bars.length * 100).toFixed(2)}%">${i === latest ? (hourly ? 'Now' : 'Today') : faDate(bar.from)}</span>`).join('');
  const grid = [[0, faCompact(scale).replace('.0', '')], [FA_HEIGHT / 2, faCompact(scale / 2).replace('.0', '')], [FA_HEIGHT, '0']].map(([at, text]) => `<div class="fa-grid${at === FA_HEIGHT ? ' base' : ''}" style="top:${at}px"><span>${text}</span></div>`).join('');
  const stats = `<div class="fa-stats"><div class="fa-top"><div class="fa-tile fa-hero"><span class="fa-lab">Requests</span><b title="${faExact(t.calls)}">${faCompact(t.calls)}</b></div><span class="fa-cap">in the ${faSpanName(span)}</span></div>
    <div class="fa-tile"><span class="fa-lab">Tokens in</span><b title="${faExact(t.input)}">${faCompact(t.input)}</b></div>
    <div class="fa-tile"><span class="fa-lab">Tokens out</span><b title="${faExact(t.output)}">${faCompact(t.output)}</b></div>
    <div class="fa-tile"><span class="fa-lab">From cache</span><b>${Math.round(t.share * 100)}%</b><span class="fa-meter"><i style="width:${Math.round(t.share * 100)}%"></i></span></div></div>`;
  const per = hourly ? 'Tokens per hour' : 'Tokens per day';
  const chart = bars.length ? `<div class="fa-block"><div class="fa-h">${per}</div>
    <div class="fa-chart" tabindex="0" role="group" aria-roledescription="chart" aria-label="${per}" aria-describedby="fa-hint">${grid}<div class="fa-days">${days}</div><div class="fa-xl">${labels}</div></div>
    <span id="fa-hint" class="fa-sr">Use arrow keys to move between days</span><div id="fa-say" class="fa-sr" aria-live="polite"></div>
    <div class="fa-readout">${faReadout(at)}</div></div>` : '';
  const models = faFold(period.models.map((row) => ({ name: row.name, value: row.tokens, calls: row.calls })), ranks);
  const kinds = window.leoChat.usageFold(period.kinds.map((row) => ({ name: row.name, value: row.calls, calls: row.calls })), ranks.kinds.slice(0, 2));
  return `${stats}${chart}<div class="fa-donuts">${faPanel('m', 'Tokens by model', models, 'tokens', 'tokens')}${faPanel('k', 'Requests by kind', kinds, 'requests', 'requests')}</div>`;
}

function faProcessIcon(name) {
  if (name === 'Codex') return ICON.logoOpenai;
  if (name === 'Claude Code') return ICON.logoClaude;
  if (name === 'Ollama') return ICON.logoOllama;
  if (name === 'Speech engine') return svg('<path d="M4 10v4M8 7v10M12 4v16M16 8v8M20 10.5v3"/>');
  if (name === 'App window') return svg('<rect x="3.5" y="5" width="17" height="14" rx="2.5"/><path d="M3.5 9h17"/>');
  return svg('<rect x="5" y="5" width="14" height="14" rx="3"/><path d="M9.5 9.5h5v5h-5z"/>');
}
function faProcessColour(name, ranks) {
  const family = name === 'Codex' ? 'openai' : name === 'Claude Code' ? 'claude' : null;
  const shades = faShades(ranks);
  const model = ranks.models.find((one) => faFamily(one) === family && shades[one]);
  return model ? shades[model] : FA_NEUTRAL;
}
function faRunning() {
  const held = felixActivity.processes;
  const ranks = felixActivity.view ? felixActivity.view.ranks : { models: [], kinds: [] };
  const refresh = `<button type="button" class="icon-btn fa-run-refresh" data-fa-refresh aria-label="Refresh" title="Refresh"${felixActivity.busy && felixActivity.roll ? ' disabled' : ''}>${ICON.refresh}</button>`;
  let top;
  let rows = '';
  let ids = '';
  if (!held) top = '<span class="set-note">Checking…</span>';
  else if (held.unavailable) top = '<span class="set-note">Couldn’t read the process list on this computer.</span>';
  else {
    if (!held.groups.length) {
      top = '<span class="set-note">No helper processes running</span>';
    } else {
      top = `<b class="fa-total">${held.total === 1 ? '1 helper process' : `${faExact(held.total)} helper processes`}</b>`;
      const most = Math.max(1, ...held.groups.map((group) => group.count));
      rows = `<div class="fa-run-rows">${held.groups.map((group) => `<div class="fa-run-row"><span class="fa-run-ic">${faProcessIcon(group.name)}</span><span class="fa-run-n" title="${esc(group.name)}">${esc(group.name)}</span><span class="fa-run-c">${faExact(group.count)}</span><span class="fa-run-bar"><i style="width:${Math.round(group.count / most * 100)}%;background:${faProcessColour(group.name, ranks)}"></i></span></div>`).join('')}</div>`;
      ids = `<details class="source-more fa-run-ids"><summary>${ICON.chevron}Show process IDs</summary><div class="fa-pids">${held.groups.map((group) => `<div><div class="fa-pid-h">${esc(group.name)}</div>${group.pids.map((one) => `<div class="fa-pid"><span>${Number(one.pid) || 0}</span><span>${esc(one.name)}</span></div>`).join('')}</div>`).join('')}</div></details>`;
    }
  }
  return `<div class="fa-h">Running now</div><div class="fa-run-top">${top}${refresh}</div>${rows}${ids}`;
}

function faKindRows(kinds) {
  const widest = Math.max(1, ...kinds.map((row) => row.p95_ms));
  return kinds.map((row) => {
    const notes = [];
    if (row.failures) notes.push(`<b>${faExact(row.failures)} failed</b>`);
    if (row.unreported) notes.push(`${faExact(row.unreported)} without token counts`);
    if (row.estimated) notes.push(`${faExact(row.estimated)} estimated`);
    return `<div class="fa-kind"><span class="fa-kn">${esc(row.label)}</span><span class="fa-kb"><span class="fa-track"><i style="width:${Math.min(100, row.median_ms / widest * 100).toFixed(1)}%"></i><u style="left:${Math.min(100, row.p95_ms / widest * 100).toFixed(1)}%"></u></span><span class="fa-kl">${faSeconds(row.median_ms)} · 95% within ${faSeconds(row.p95_ms)}</span></span><span class="fa-kc">${faCompact(row.calls)} ${row.calls === 1 ? 'call' : 'calls'}</span><span class="fa-kx">${notes.join(' · ')}</span></div>`;
  }).join('');
}

function felixUsageInner(page) {
  const chat = window.leoChat;
  const today = faToday();
  const span = felixActivity.span;
  const ranks = faRanks(page, today);
  const recent = page.felix_recent || {};
  const period = span === 1 ? chat.usagePeriod(recent.hours, 1, faToday(true)) : chat.usagePeriod(page.felix_days, span, today);
  felixActivity.view = { period, ranks };
  const detail = (recent.spans && recent.spans[span]) || {};
  const kinds = chat.usageKinds(detail.kinds);
  const tools = detail.tools || {};
  const empty = !period.totals.calls && !period.models.length;
  const name = faSpanName(span);
  return `<header><h3>Felix activity</h3>${segmented({ label: 'Activity period', choices: [['1', '24 hours'], ['7', '7 days'], ['30', '30 days'], ['90', '90 days']], current: String(span), field: 'id="fa-span"' })}</header>
    <div class="fa-body" role="group" aria-label="${name[0].toUpperCase()}${name.slice(1)}">${empty ? `<p class="set-note">No AI requests in the ${name}.</p>` : faPeriod(period, ranks, span)}</div>
    <div class="fa-block fa-run">${faRunning()}</div>
    <details class="source-more fa-more"><summary>${ICON.chevron}More detail</summary><div class="fa-block"><div class="fa-h">In the ${name}</div>
      ${kinds.length ? `<div class="fa-kinds">${faKindRows(kinds)}</div>` : ''}
      <div class="fa-tools"><span class="fa-lab">Tools</span><span>${faCompact(Number(tools.calls) || 0)} calls · ${faCompact(Number(tools.reused) || 0)} unchanged reads reused · ${faCompact(Number(tools.failures) || 0)} failed</span></div>
      <p class="fa-foot">Cache is part of input, not your subscription balance. Provider retries may be unreported.</p></div></details>`;
}

function felixUsageCard(page) {
  return `<section class="set-card fa-card" data-find="writing-activity">${felixUsageInner(page)}</section>`;
}

function faHighlight(card, kind, index) {
  const panel = card.querySelector(`.fa-panel[data-fa-d="${kind}"]`);
  if (!panel || !panel.querySelector('.fa-ring')) return;
  const pin = felixActivity.pin && felixActivity.pin.d === kind ? felixActivity.pin.i : null;
  const shown = index === null ? pin : index;
  panel.querySelector('.fa-ring').classList.toggle('hl', shown !== null);
  for (const slice of panel.querySelectorAll('.fa-slice')) slice.classList.toggle('on', Number(slice.dataset.faI) === shown);
  for (const row of panel.querySelectorAll('.fa-lg')) {
    row.classList.toggle('on', Number(row.dataset.faI) === shown);
    row.setAttribute('aria-pressed', String(Number(row.dataset.faI) === pin));
  }
  const centre = panel.querySelector('.fa-centre');
  const row = shown === null ? null : panel.querySelector(`.fa-lg[data-fa-i="${shown}"]`);
  centre.querySelector('b').textContent = row ? row.dataset.big : centre.dataset.big;
  centre.querySelector('span').textContent = row ? row.dataset.small : centre.dataset.small;
}
function faEscape(e) {
  if (!felixActivity.pin) {
    document.removeEventListener('keydown', faEscape, true);
    return;
  }
  if (e.key !== 'Escape' || state.view !== 'settings' || state.section !== 'writing' || document.querySelector('.scrim') || !app.querySelector('[data-find="writing-activity"]')) return;
  e.stopPropagation();
  e.preventDefault();
  faPin(null);
}
function faPin(next) {
  const was = felixActivity.pin;
  felixActivity.pin = next;
  if (next && !was) document.addEventListener('keydown', faEscape, true);
  if (!next) document.removeEventListener('keydown', faEscape, true);
  const card = app.querySelector('[data-find="writing-activity"]');
  if (!card) return;
  for (const kind of ['m', 'k']) faHighlight(card, kind, null);
}
function faShowDay(card, index, keys) {
  const days = [...card.querySelectorAll('.fa-day')];
  if (!days[index]) return;
  days.forEach((day, i) => day.classList.toggle('on', i === index));
  const readout = card.querySelector('.fa-readout');
  readout.innerHTML = faReadout(index);
  if (keys) {
    felixActivity.day = index;
    card.querySelector('#fa-say').textContent = readout.textContent;
  }
}
function faNearest(chart, x) {
  const days = chart.querySelector('.fa-days').getBoundingClientRect();
  const count = chart.querySelectorAll('.fa-day').length;
  return Math.max(0, Math.min(count - 1, Math.floor((x - days.left) / Math.max(1, days.width) * count)));
}
function faSettledDay(card) {
  const count = card.querySelectorAll('.fa-day').length;
  return felixActivity.day !== null && felixActivity.day < count ? felixActivity.day : count - 1;
}

function bindActivity(card) {
  if (!card || faBound.has(card)) return;
  faBound.add(card);
  const lens = (e) => e.target.closest && e.target.closest('[data-fa-i]');
  card.addEventListener('pointerover', (e) => {
    const hit = lens(e);
    if (hit) faHighlight(card, hit.dataset.faD, Number(hit.dataset.faI));
  });
  card.addEventListener('pointerout', (e) => {
    const hit = lens(e);
    if (hit && !hit.contains(e.relatedTarget)) faHighlight(card, hit.dataset.faD, null);
  });
  card.addEventListener('focusin', (e) => {
    const hit = lens(e);
    if (hit && hit.matches('.fa-lg')) faHighlight(card, hit.dataset.faD, Number(hit.dataset.faI));
  });
  card.addEventListener('focusout', (e) => {
    const hit = lens(e);
    if (hit && hit.matches('.fa-lg')) faHighlight(card, hit.dataset.faD, null);
  });
  card.addEventListener('click', (e) => {
    const row = e.target.closest('.fa-lg');
    if (row) {
      const same = felixActivity.pin && felixActivity.pin.d === row.dataset.faD && felixActivity.pin.i === Number(row.dataset.faI);
      faPin(same ? null : { d: row.dataset.faD, i: Number(row.dataset.faI) });
      return;
    }
    if (e.target.closest('[data-fa-refresh]')) loadProcesses(true);
  });
  card.addEventListener('pointermove', (e) => {
    const chart = e.target.closest && e.target.closest('.fa-chart');
    if (chart) faShowDay(card, faNearest(chart, e.clientX), false);
  });
  card.addEventListener('pointerleave', () => {
    if (card.querySelector('.fa-chart')) faShowDay(card, faSettledDay(card), false);
  });
  card.addEventListener('keydown', (e) => {
    const chart = e.target.closest && e.target.closest('.fa-chart');
    if (!chart) return;
    const last = card.querySelectorAll('.fa-day').length - 1;
    const now = faSettledDay(card);
    const next = { ArrowLeft: now - 1, ArrowRight: now + 1, Home: 0, End: last }[e.key];
    if (next === undefined) return;
    e.preventDefault();
    e.stopPropagation();
    faShowDay(card, Math.max(0, Math.min(last, next)), true);
  });
}

function redrawRunning(roll) {
  const box = app.querySelector('.fa-run');
  if (!box || !felixActivity.view) return;
  refreshPanel(box, faRunning());
  if (roll && !stillMotion()) {
    const total = box.querySelector('.fa-total');
    if (total) total.animate([{ opacity: 0, transform: 'translateY(6px)' }, { opacity: 1, transform: 'none' }], { duration: MOTION_MS, easing: EASE_OUT });
  }
}
async function loadProcesses(user) {
  if (felixActivity.busy) return;
  felixActivity.busy = true;
  felixActivity.roll = user;
  if (user) redrawRunning(false);
  try {
    felixActivity.processes = await api('/api/processes');
  } catch (e) {
    if (!felixActivity.processes) felixActivity.processes = { total: 0, groups: [], unavailable: true };
  }
  felixActivity.busy = false;
  redrawRunning(user);
  felixActivity.roll = false;
}
function stopActivity() {
  if (felixActivity.pin) faPin(null);
  felixActivity.watch?.disconnect();
  felixActivity.watch = null;
  if (!felixActivity.watching) return;
  felixActivity.watching = false;
  clearInterval(felixActivity.timer);
  window.removeEventListener('leo-visibility', activityVisible);
  document.removeEventListener('visibilitychange', activityVisible);
}
function activityVisible() {
  if (felixActivity.watching && appVisible()) loadProcesses(false);
}
function tickProcesses() {
  if (state.view !== 'settings' || state.section !== 'writing') stopActivity();
  else if (appVisible()) loadProcesses(false);
}
function playActivity(card) {
  if (felixActivity.drawn) return;
  if (stillMotion() || !card.querySelector('.fa-days')) {
    if (card.querySelector('.fa-days')) felixActivity.drawn = true;
    return;
  }
  const run = () => {
    if (felixActivity.drawn || !card.isConnected) return;
    felixActivity.drawn = true;
    for (const sweep of card.querySelectorAll('.fa-sweep')) sweep.animate([{ strokeDashoffset: FA_RING }, { strokeDashoffset: 0 }], { duration: OPEN_MS, easing: EASE_OUT });
    card.querySelector('.fa-days').animate([{ opacity: 0 }, { opacity: 1 }], { duration: OPEN_MS, easing: EASE_OUT });
  };
  if (typeof IntersectionObserver !== 'function') return run();
  felixActivity.watch?.disconnect();
  const watch = new IntersectionObserver((entries) => {
    if (!entries.some((entry) => entry.isIntersecting)) return;
    watch.disconnect();
    felixActivity.watch = null;
    run();
  });
  felixActivity.watch = watch;
  watch.observe(card);
}
function activityShown(section) {
  if (section !== 'writing') return stopActivity();
  const card = app.querySelector('[data-find="writing-activity"]');
  if (!card) return;
  bindActivity(card);
  playActivity(card);
  if (felixActivity.watching) return;
  felixActivity.watching = true;
  window.addEventListener('leo-visibility', activityVisible);
  document.addEventListener('visibilitychange', activityVisible);
  felixActivity.timer = setInterval(tickProcesses, 10000);
  loadProcesses(false);
}

document.addEventListener('change', (e) => {
  if (!e.target || e.target.id !== 'fa-span' || state.view !== 'settings' || !state.settings) return;
  const span = Number(e.target.value);
  if (![1, 7, 30, 90].includes(span) || span === felixActivity.span) return;
  felixActivity.span = span;
  felixActivity.day = null;
  if (felixActivity.pin) faPin(null);
  const card = app.querySelector('[data-find="writing-activity"]');
  if (!card) return;
  refreshPanel(card, felixUsageInner(state.settings));
  bindActivity(card);
  const body = card.querySelector('.fa-body');
  if (body && !stillMotion()) body.animate([{ opacity: 0.35 }, { opacity: 1 }], { duration: MOTION_MS, easing: EASE_OUT });
});

const shortcutRec = { name: '', on: false, held: '', hint: '', bad: false };
const SHORTCUT_RESERVED = { q: 'quit', w: 'closing the window', r: 'reload', c: 'copy', v: 'paste', x: 'cut', z: 'undo', y: 'redo', a: 'select all', s: 'save', b: 'bold', i: 'italic', u: 'underline', p: 'print', h: 'hiding leo', m: 'minimizing', ',': 'settings' };
function shortcutRow(name, find) {
  const mine = shortcutRec.name === name, on = mine && shortcutRec.on, label = SHORTCUTS[name][0];
  const hint = on ? 'Esc to cancel' : mine ? shortcutRec.hint : '';
  return `<div class="set-row column" data-find="${find}"><div class="set-line"><span class="set-label">${label}</span><span class="set-shortcut"><span class="shortcut-field${on ? ' recording' : ''}" role="group" aria-label="${label} shortcut">${dictationShortcutField({ recording: on, held: shortcutRec.held }, shortcutKeys(name))}</span>${shortcutKeys(name) !== shortcutDefault(name) ? `<button class="btn plain sm" data-action="shortcut-reset" data-name="${name}" aria-label="Reset ${label} shortcut">Reset</button>` : ''}<button class="btn plain sm" data-action="shortcut-record" data-name="${name}" aria-label="Change ${label} shortcut">Change</button></span></div><span class="hint${mine && shortcutRec.bad && !on ? ' bad' : ''}" aria-live="polite">${esc(hint)}</span></div>`;
}
function shortcutsCard() {
  return `<section class="set-card"><header><h3>Keyboard shortcuts</h3></header><p class="hint">Click Change, then press the new keys. Esc cancels.</p>
    ${shortcutRow('quick', 'shortcut-quick')}${shortcutRow('new', 'shortcut-new')}${shortcutRow('ask', 'shortcut-ask')}
    ${state.dictation ? dictationShortcutRow(state.dictation) : ''}
  </section>`;
}
function shortcutRefusal(name, chord) {
  const mod = ON_MAC ? 'cmd' : 'ctrl', parts = chord.split('+'), text = shortcutText(chord);
  if (!parts.includes(mod) && !(ON_MAC && parts.includes('ctrl'))) return `Use ${ON_MAC ? '⌘' : 'Ctrl'} with a key.`;
  const reserved = parts[0] === mod && parts.length === 2 ? SHORTCUT_RESERVED[parts[1]] : chord === `${mod}+shift+z` ? 'redo' : '';
  if (reserved) return `${text} is reserved for ${reserved}.`;
  const other = Object.keys(SHORTCUTS).find((key) => key !== name && shortcutKeys(key) === chord);
  if (other) return `Already used by ${SHORTCUTS[other][0]}.`;
  const dictation = state.dictation?.settings?.shortcut;
  return dictation && shortcutText(dictation) === text ? 'Already used by Dictation.' : '';
}
function shortcutSave(name, chord) {
  try {
    const all = JSON.parse(localStorage.getItem('leo-shortcuts')) || {};
    if (chord) all[name] = chord; else delete all[name];
    localStorage.setItem('leo-shortcuts', JSON.stringify(all));
    return '';
  } catch (e) {
    return 'This browser would not save the shortcut.';
  }
}
function shortcutStop() {
  window.removeEventListener('keydown', shortcutKey, true);
  window.removeEventListener('pointerdown', shortcutAway, true);
  window.removeEventListener('hashchange', shortcutStop);
  shortcutRec.on = false;
}
function shortcutDrawn() {
  drawSettings(state.settings);
  $(`[data-action="shortcut-record"][data-name="${shortcutRec.name}"]`)?.focus();
}
function shortcutCancel() {
  shortcutRec.hint = '';
  shortcutStop();
  shortcutDrawn();
}
function shortcutKey(e) {
  e.preventDefault();
  e.stopImmediatePropagation();
  if (e.key === 'Escape') return shortcutCancel();
  const chord = chordOf(e);
  if (!chord) {
    const held = chordOf(e, true);
    if (held !== shortcutRec.held) { shortcutRec.held = held; drawSettings(state.settings); }
    return;
  }
  const why = shortcutRefusal(shortcutRec.name, chord) || shortcutSave(shortcutRec.name, chord);
  Object.assign(shortcutRec, { hint: why || 'Saved', bad: !!why });
  shortcutStop();
  shortcutDrawn();
  drawSide();
}
function shortcutAway(e) {
  if (e.target.closest('[data-action^="shortcut-"]')) shortcutStop();
  else if (!e.target.closest('.shortcut-field.recording')) shortcutCancel();
}
function shortcutRecord(name) {
  shortcutStop();
  Object.assign(shortcutRec, { name, on: true, held: '', hint: '', bad: false });
  window.addEventListener('keydown', shortcutKey, true);
  window.addEventListener('pointerdown', shortcutAway, true);
  window.addEventListener('hashchange', shortcutStop);
  drawSettings(state.settings);
}
function shortcutReset(name) {
  shortcutStop();
  const why = shortcutSave(name, '');
  Object.assign(shortcutRec, { name, held: '', hint: why || 'Back to the default', bad: !!why });
  drawSettings(state.settings);
  drawSide();
}

function drawSettings(page) {
  const fields = keepSettingsFields(), segments = keepSegmented();
  state.settings = page;
  const section = state.section || 'general';
  let content = '';
  if (section === 'general') content = themeCard() + `<div id="app-general">${nativeGeneralCard(desktopApp)}</div>`;
  else if (section === 'shortcuts') content = shortcutsCard();
  else if (section === 'dictation') content = `<div id="dictation-library">${dictationCards(state.dictation) || '<section class="set-card">Open the leo app to set up dictation.</section>'}</div>`;
  else if (section === 'models') content = `<div id="speech-library">${speechCards(state.speech)}</div>`;
  else if (section === 'writing') content = page.tasks.filter((task) => task.task === 'writing').map(taskCard).join('') + felixUsageCard(page);
  else if (section === 'felix') content = felixSettingsCards(state.margin, page);
  else if (section === 'calendar') content = calendarCard(state.calendar);
  else if (section === 'health') content = `<div id="app-health">${healthCards(state.health)}</div>`;
  else if (section === 'updates') content = `<div id="app-updates">${updatesCard(desktopApp)}</div>`;
  else content = '';
  refreshPanel(app, settingsFrame(section, `<div class="settings">${content}</div>`));
  restoreSettingsFields(fields, segments);
  activityShown(section);
  if ($('#ollama-setup')) loadOllama(seq);
}

async function changeSetting(change) {
  const mine = seq;
  try {
    const done = await api('/api/settings', { method: 'POST', body: change });
    if (mine !== seq || state.view !== 'settings') return;
    drawSettings(done.settings);
    toast(done.message);
  } catch (e) {
    if (mine === seq && state.view === 'settings' && state.settings) drawSettings(state.settings);
    throw e;
  }
}

app.addEventListener('change', (e) => {
  const el = e.target.closest('select[data-set],input[data-set]');
  if (!el || state.view !== 'settings') return;
  const change = { set: el.dataset.set, value: el.value };
  if (el.dataset.task) change.task = el.dataset.task;
  changeSetting(change).catch(fail);
});

app.addEventListener('keydown', (e) => {
  const input = e.target.closest('[data-key-input]');
  if (input && e.key === 'Enter') {
    e.preventDefault();
    saveKey(input.dataset.keyInput).catch(fail);
  }
});

async function saveKey(account) {
  const input = app.querySelector(`[data-key-input="${CSS.escape(account)}"]`);
  const value = input ? input.value.trim() : '';
  if (!value) return toast('Paste the key first.', { bad: true });
  input.value = '';
  await changeSetting({ set: 'key', account, value });
}

function confirmRemoveKey(el) {
  const { name, account } = el.dataset;
  popupMenu(el, {
    label: `Remove the ${name} key`,
    rows: [
      { note: `leo stops using ${name} until you add a key again.` },
      { text: 'Remove key', icon: ICON.trash, danger: true, run: () => changeSetting({ set: 'key', account, value: null }) },
    ],
  });
}

async function testAi(el) {
  const out = $(`#test-${el.dataset.task}`);
  el.disabled = true;
  out.className = 'set-result';
  out.textContent = 'Asking…';
  try {
    const done = await api('/api/settings/test', { method: 'POST', body: { task: el.dataset.task } });
    out.className = 'set-result ok';
    out.textContent = done.message;
  } catch (e) {
    out.className = 'set-result bad';
    out.textContent = e.message;
  } finally {
    el.disabled = false;
  }
}

function speechCards(view) {
  if (!view) return '<section class="set-card"><p class="hint">Speech models could not be opened. Reopen leo and try again.</p></section>';
  const selected = view.selected || {};
  const current = view.models.find((model) => model.id === selected.id);
  const language = current && view.ready && current.languages.length > 1 ? `<label class="set-row" data-find="speech-language"><span class="set-label">Language</span><span class="select-control"><select id="speech-language" data-speech-language="${esc(current.id)}">${current.detects_language === false ? '' : settingsOption('auto', 'Detect language', selected.language)}${current.languages.map((code) => settingsOption(code, speechLanguage(code), selected.language)).join('')}</select>${ICON.chevron}</span></label>` : '';
  const downloaded = view.models.filter((model) => model.downloaded);
  const extras = view.extra_files || [];
  const available = view.models.filter((model) => !model.downloaded);
  if (!state.speechFilter || state.speechFilter === 'downloaded') state.speechFilter = 'recommended';
  const filter = state.speechFilter;
  const filters = [['recommended', 'Recommended'], ['english', 'English'], ['many', 'Multilingual'], ['all', `All ${available.length}`]];
  const rows = speechFiltered(available, filter).map(speechModelRow).join('');
  return `<section class="set-card" data-find="speech-in-use"><header><h3>In use</h3><span class="set-status ${view.ready ? 'ok' : 'missing'}">${view.ready ? 'Ready' : 'Choose a model'}</span></header><div class="set-row"><span class="grow set-value-text">${view.ready && current ? esc(current.name) : 'Download a model below, then choose Use.'}</span></div><p class="set-note">Dictation and recording share this model. Audio stays on this computer.</p>${language}</section>
    <section class="set-card" data-models-downloaded data-find="speech-downloaded"><header><h3>Downloaded</h3><span class="hint">${plural(downloaded.length + extras.length, 'model')} · ${speechBytes(view.disk_bytes)} on this computer</span></header>
      ${downloaded.map(speechModelRow).join('')}${extras.map((file) => `<div class="set-row"><span class="grow"><span>${esc(file.name)}</span><span class="set-note">${speechBytes(file.bytes)} · Not in the current catalog</span></span><button class="btn plain sm" data-action="speech-model" data-model="${esc(file.name)}" data-verb="delete_file">Delete</button></div>`).join('')}${!downloaded.length && !extras.length ? '<p class="set-note">No models downloaded yet.</p>' : ''}</section>
    <section class="set-card" data-models-all data-find="speech-all"><header><h3>All models</h3></header><p class="hint">Choose a local model. Larger models use more memory.</p><div class="model-filters">${segmented({ label: 'Filter models', action: 'speech-filter', key: 'filter', current: filter, choices: filters })}</div>${rows || '<p class="set-note">No models match this filter.</p>'}<p class="set-note">Accuracy uses the chosen language, or combines measured languages with Detect language. Benchmark speed may differ on your computer.</p></section>`;
}
function speechBytes(bytes) {
  bytes = Math.max(0, Number(bytes) || 0);
  return bytes >= 1073741824 ? (bytes / 1073741824).toFixed(1) + ' GB' : bytes >= 1048576 ? Math.round(bytes / 1048576) + ' MB' : bytes + ' B';
}
function speechUseLanguage(model) {
  if (model.detects_language !== false) return 'auto';
  const mine = String(globalThis.navigator?.language || '').split('-')[0].toLowerCase();
  return model.languages.includes(mine) ? mine : model.languages.includes('en') ? 'en' : model.languages[0];
}
function speechLanguage(code) {
  try { return new Intl.DisplayNames(['en'], { type: 'language' }).of(code) || code; } catch (e) { return code; }
}
function speechFiltered(models, filter) {
  return models.filter((model) => filter === 'all' || filter === 'recommended' && model.recommended || filter === 'english' && model.languages.includes('en') || filter === 'many' && model.languages.length > 1);
}
function speechRatings(model) {
  const speed = Math.max(0, Number(model.speed) || 0);
  const accuracy = Math.max(0, Math.min(100, Number(model.accuracy) || 0));
  const measured = model.accuracy_measured ?? (accuracy > 0);
  const language = model.benchmark_language && model.benchmark_language !== 'auto' ? speechLanguage(model.benchmark_language) : 'Measured languages';
  const wait = speed > 0 ? 30 / speed : null;
  const level = wait === null ? 0 : [2, 3.5, 7.5, 20].filter(cutoff => wait < cutoff).length;
  const speedLabel = speed > 0 ? speed.toFixed(1) + '× real time' : 'Not measured';
  const accuracyLabel = measured ? accuracy.toFixed(1) + '%' : 'Not measured';
  return `<div class="model-ratings"><span class="model-rating" title="${esc(language)} benchmark"><span>Accuracy</span>${measured ? `<meter class="model-meter" min="0" max="100" value="${accuracy}" aria-label="Accuracy ${esc(accuracyLabel)}"></meter>` : ''}<span class="hint">${esc(accuracyLabel)}</span></span><span class="model-rating" title="${wait === null ? 'No speed benchmark' : 'Estimated wait after 30 seconds of speech: ' + wait.toFixed(1) + ' seconds'}"><span>Speed</span>${speed > 0 ? `<meter class="model-meter" min="0" max="4" value="${level}" aria-label="Speed ${esc(speedLabel)}"></meter>` : ''}<span class="hint">${esc(speedLabel)}</span></span></div>`;
}
function speechModelRow(model) {
  const progress = model.progress;
  const downloading = progress && progress.state === 'downloading';
  const percent = downloading ? Math.min(100, Math.floor(progress.bytes * 100 / Math.max(1, progress.total))) : 0;
  const action = (verb, label) => `<button class="btn plain sm" data-action="speech-model" data-model="${esc(model.id)}" data-verb="${verb}">${label}</button>`;
  const controls = downloading ? `<span class="hint">${percent}%</span><progress value="${percent}" max="100" aria-label="Downloading ${esc(model.name)}"></progress>${action('cancel', 'Cancel')}` : model.verified ? `${model.in_use ? '<span class="chip">In use</span>' : action('use', 'Use')}${action('delete', 'Delete')}` : `${model.downloaded ? '<span class="chip">Check failed</span>' : ''}${action('download', model.downloaded ? 'Download again' : 'Download')}${model.downloaded ? action('delete', 'Delete') : ''}`;
  const names = model.languages.map(speechLanguage);
  return `<div class="set-row column" data-model-row="${esc(model.id)}"><div class="set-line"><span class="set-label">${esc(model.name)}${model.recommended ? ' <span class="chip">Recommended</span>' : ''}</span><span class="set-value">${controls}</span></div>${model.description ? `<p class="hint">${esc(model.description)}</p>` : ''}<span class="hint" title="${esc(names.join(', '))}">${esc(names.length === 1 ? names[0] + ' only' : names.length + ' languages')} · ${speechBytes(model.size)} · ${model.streaming ? 'Streaming' : 'Text after you speak'} · ${model.live ? 'Live lecture transcripts' : 'Lecture transcript after recording'}</span>${speechRatings(model)}${progress && progress.error && progress.state !== 'cancelled' ? `<span class="set-note warn">${esc(progress.error)}</span>` : ''}</div>`;
}
function filterSpeech(el) {
  const box = $('#speech-library');
  const from = Number(box?.querySelector('.model-filters .segmented')?.style.getPropertyValue('--at'));
  state.speechFilter = el.dataset.filter;
  if (!box) return;
  const section = box.closest('.settings-section') || box.parentElement;
  const before = box.querySelector('.model-filters')?.getBoundingClientRect().top;
  const top = scrollY;
  refreshPanel(box, speechCards(state.speech));
  if (section) {
    const short = Math.ceil(top + innerHeight - document.documentElement.scrollHeight);
    if (short > 0) section.style.minHeight = `${section.offsetHeight + short}px`;
  }
  const after = box.querySelector('.model-filters')?.getBoundingClientRect().top;
  if (before != null && after != null) scrollBy(0, after - before);
  const filters = box.querySelector('.model-filters .segmented');
  if (filters) syncSegmented(filters, Number.isFinite(from) ? from : null);
  filters?.querySelector(`[data-filter="${CSS.escape(state.speechFilter)}"]`)?.focus({ preventScroll: true });
}

let speechPollGeneration = 0;
function startSpeechPolling(mine) { pollSpeech(mine, ++speechPollGeneration); }

async function pollSpeech(mine, generation) {
  if (generation !== speechPollGeneration || mine !== seq || state.view !== 'settings' || !appVisible()) return;
  if (!(state.speech && state.speech.models.some((m) => m.progress && m.progress.state === 'downloading'))) return;
  await new Promise((resolve) => setTimeout(resolve, 1000));
  if (generation !== speechPollGeneration || mine !== seq || state.view !== 'settings' || !appVisible()) return;
  try {
    const view = await api('/api/speech');
    if (generation !== speechPollGeneration || mine !== seq || state.view !== 'settings') return;
    state.speech = view;
    const box = $('#speech-library');
    refreshPanel(box, speechCards(view));
  } catch (e) { if (mine !== seq) return; }
  pollSpeech(mine, generation);
}

async function speechChange(change) {
  const mine = seq;
  const generation = ++speechPollGeneration;
  const view = await api('/api/speech', { method: 'POST', body: change });
  if (generation !== speechPollGeneration || mine !== seq || state.view !== 'settings') return;
  state.speech = view;
  const box = $('#speech-library');
  refreshPanel(box, speechCards(view));
  pollSpeech(mine, generation);
}

function speechDeleteMenu(el, name, note, change) {
  popupMenu(el, { label: `Delete ${name}`, rows: [{ note }, { text: 'Delete', icon: ICON.trash, danger: true, run: () => speechChange(change) }] });
}

function changeSpeechModel(el) {
  if (el.dataset.verb === 'delete_file') {
    const file = (state.speech.extra_files || []).find((file) => file.name === el.dataset.model);
    if (file) speechDeleteMenu(el, file.name, 'This removes the model file from this computer.', { action: 'delete_file', id: file.name });
    return;
  }
  const model = state.speech.models.find((m) => m.id === el.dataset.model);
  if (!model) return;
  const change = { action: el.dataset.verb, id: model.id, language: model.languages.length === 1 ? model.languages[0] : speechUseLanguage(model) };
  if (change.action === 'delete') return speechDeleteMenu(el, model.name, 'The download is removed from this computer. Your notes stay.', { action: 'delete', id: model.id });
  return speechChange(change);
}

app.addEventListener('change', (event) => {
  const language = event.target.closest('[data-speech-language]');
  if (language) speechChange({ action: 'use', id: language.dataset.speechLanguage, language: language.value }).catch(fail);
});

async function showUsage(trigger = null) {
  const opener = trigger?.closest?.('.scrim') ? $('#menu') : trigger || document.activeElement;
  const plan = `<section class="coding-plan">${skeleton(3, 'plan')}</section>`;
  const box = sheet(`<header class="usage-head"><div><h3>Usage</h3><p class="usage-sub">Plan limits</p></div><div class="usage-tools"><button type="button" class="icon-btn" data-action="usage-refresh" aria-label="Refresh" title="Refresh" disabled>${ICON.refresh}</button><button type="button" class="icon-btn" data-action="close" aria-label="Close" title="Close">${ICON.close}</button></div></header><div class="usage-content" aria-busy="true"><div class="coding-plans" aria-hidden="true">${plan}${plan}</div></div>`);
  box.returnFocus = opener;
  box.querySelector('.sheet').classList.add('usage-sheet');
  box.querySelector('[data-action="close"]').focus();
  const mine = seq;
  let saved = false, refreshing = true, drawn = false;
  const live = () => mine === seq && box.isConnected && !box.classList.contains('scrim-gone');
  const content = box.querySelector('.usage-content');
  const button = box.querySelector('[data-action="usage-refresh"]');
  const busy = (on) => {
    refreshing = on;
    button.disabled = on;
    button.innerHTML = on ? '<span class="activity-spin"></span>' : ICON.refresh;
  };
  const draw = (page) => {
    content.removeAttribute('aria-busy');
    content.innerHTML = (codingPlans(page) || '<p class="hint" role="status">No coding accounts detected.</p>') + (page.note ? `<p class="hint usage-note">${esc(page.note)}</p>` : '');
    if (!drawn) cascade(content.querySelector('.coding-plans'));
    drawn = true;
  };
  const refresh = async (body = {}) => {
    if (refreshing || !live()) return;
    const focused = document.activeElement === button;
    busy(true);
    try {
      const fresh = await api('/api/settings/usage', { method: 'POST', body });
      if (!live()) return;
      draw(fresh);
      saved = true;
    } catch (e) {
      if (!live()) return;
      if (!saved) { content.removeAttribute('aria-busy'); content.innerHTML = '<p class="hint" role="status">Usage could not be loaded.</p>'; }
      else {
        content.querySelector('.usage-error')?.remove();
        content.insertAdjacentHTML('beforeend', '<p class="hint usage-error" role="status">Refresh failed · showing saved usage</p>');
      }
    } finally {
      if (live()) {
        busy(false);
        if (focused && document.activeElement === document.body) button.focus();
      } else refreshing = false;
    }
  };
  box.addEventListener('click', event => {
    if (event.target.closest('[data-action="usage-refresh"]') && !refreshing) { button.focus(); refresh({ claude: true }); }
  });
  try {
    const page = await api('/api/settings');
    if (!live()) return;
    draw(page);
    saved = Boolean(page.plans?.length);
  } catch (e) {
    if (!live()) return;
    content.removeAttribute('aria-busy');
    content.innerHTML = '<p class="hint" role="status">Usage could not be loaded.</p>';
  }
  busy(false);
  refresh();
}
