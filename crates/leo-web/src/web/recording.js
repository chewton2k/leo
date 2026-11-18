(function (root) {
  'use strict';

  const WANTS_SAVE_AFTER = 900;
  const POLL_EVERY = 1000;

  function clock(secs) {
    const s = Math.max(0, Math.floor(secs || 0));
    const h = Math.floor(s / 3600);
    const m = Math.floor((s % 3600) / 60);
    const pad = (n) => String(n).padStart(2, '0');
    return h ? `${h}:${pad(m)}:${pad(s % 60)}` : `${m}:${pad(s % 60)}`;
  }

  const QUIET = 0.2;
  const WAVE_STEP_MS = 60;
  const SERVER_STEP_MS = 250;
  const MOST_WAVE = 400;
  const MOST_LEVEL_QUEUE = 40;
  const BAR = 3;
  const GAP = 2.5;

  function ease(current, target) {
    const rate = target > current ? 0.55 : 0.16;
    return current + (target - current) * rate;
  }

  function spread(from, to, steps) {
    return Array.from({ length: steps }, (_, i) => from + ((to - from) * (i + 1)) / steps);
  }

  function freshLevels(levels, start, fedUpTo) {
    const end = start + levels.length;
    if (fedUpTo >= end) return { fresh: [], fedUpTo };
    const from = Math.max(fedUpTo, start);
    return { fresh: levels.slice(from - start), fedUpTo: end };
  }

  function sourceFor(kind) {
    return kind === 'microphone' ? 'microphone' : kind === 'call' ? 'nativecall' : 'screen';
  }

  function loudness(rms) {
    if (!(rms > 0)) return 0;
    const db = 20 * Math.log10(rms);
    return Math.max(0, Math.min(1, (db + 54) / 48));
  }

  function hearing(levels, stepMs) {
    if (!levels.length) return 'waiting';
    let quietMs = 0;
    for (let i = levels.length - 1; i >= 0 && levels[i] < QUIET; i--) quietMs += stepMs;
    if (quietMs < 1500) return 'sound';
    return quietMs < 6000 ? 'quiet' : 'silent';
  }

  function hearingWords(state, source, paused, denied) {
    if (paused) return 'Paused: nothing is being recorded';
    if (denied && source !== 'screen') return 'leo can\u2019t hear: microphone access is off.';
    if (state === 'sound') return 'Hearing sound';
    if (state === 'waiting' || state === 'quiet') return 'Listening…';
    if (source === 'screen') return 'No sound for a while: is something playing on the computer?';
    return 'No sound for a while: is the right microphone chosen, and not muted?';
  }

  function stateWord(view) {
    if (!view) return '';
    if (view.state === 'paused') return 'Paused';
    if (view.state === 'recording') return 'Recording';
    if (view.state === 'starting') return 'Starting';
    if (view.state === 'writing') return view.steps ? `${view.step} ${view.steps[0]}/${view.steps[1]}` : view.step;
    if (view.state === 'done') return 'Saved';
    if (view.state === 'failed') return 'Stopped';
    return view.state;
  }

  const live = (view) => Boolean(view) && ['starting', 'recording', 'paused'].includes(view.state);

  const MEETING_WANTS = 'Meeting notes: what was discussed, the decisions made, open questions, and action items with owners and due dates when they were said.';
  const SOON_MS = 15 * 60 * 1000;

  const HEADSET = 'Your sound output is a headset, so leo can\u2019t record only what this computer plays. Switch to other speakers or headphones first.';

  function preflight(checks) {
    const c = checks || {};
    return [
      c.speech === false && { text: 'No speech model yet. The audio is kept and written up once you choose one.', href: '#/settings/models', link: 'Choose a speech model' },
      c.writing === false && { text: 'No writing AI is ready, so you\u2019ll get a transcript.', href: '#/settings/writing', link: 'Set up writing AI' },
    ].filter(Boolean);
  }

  const tipWords = (platform) => `Keeps recording if you close the window. Open leo from the ${platform === 'windows' ? 'system tray' : 'menu bar'} to return.`;

  function eventNow(events, now = Date.now()) {
    return (events || []).findIndex((e) => Date.parse(e.start) - SOON_MS <= now && now <= Date.parse(e.end));
  }

  function whenOf(e) {
    const start = new Date(e.start);
    const end = new Date(e.end);
    const day = start.toDateString() === new Date().toDateString() ? 'Today' : start.toLocaleDateString(undefined, { weekday: 'short', month: 'short', day: 'numeric' });
    const time = (d) => d.toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' });
    return `${day} ${time(start)}–${time(end)}`;
  }

  const PENDING_KINDS = { microphone: 'Microphone', screen: 'Screen', nativecall: 'Call' };

  function pendingLine(p, now = new Date()) {
    const at = p.started ? new Date(p.started) : null;
    const when = at && !Number.isNaN(at.getTime())
      ? `${at.toDateString() === now.toDateString() ? 'Today' : at.toLocaleDateString(undefined, { weekday: 'short', month: 'short', day: 'numeric' })} ${at.toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' })}`
      : '';
    return [when, PENDING_KINDS[p.source], p.directory || 'Top level'].filter(Boolean).join(' · ');
  }

  function mergePages(shown, page) {
    const ids = new Set((shown.pending || []).map((p) => p.id));
    return { ...page, pending: [...(shown.pending || []), ...(page.pending || []).filter((p) => !ids.has(p.id))], more: page.more, after: page.after };
  }

  function earlierFirst(earlier, shown) {
    const seen = new Set();
    return [...earlier, ...shown].filter(([at]) => !seen.has(at) && seen.add(at)).sort((a, b) => a[0] - b[0]);
  }

  function placed(view) {
    const points = view.points || [];
    const first = (view.points_total ?? points.length) - points.length;
    return points.map(([at, text], k) => [first + k, at, text]);
  }

  function unsavedLine(n, more) {
    if (more) return `${n} or more recordings weren't saved`;
    return n === 1 ? "1 recording wasn't saved" : `${n} recordings weren't saved`;
  }

  function failureOf(view) {
    const error = typeof view.error === 'string' ? view.error.slice(0, 12000) : '';
    const silent = Boolean(error) && error.split('; ').every((part) => /^(?:You: |Others: )?No sound was recorded\./.test(part));
    const details = [...new Set([error, ...(view.warnings || []).slice(0, 20)].filter(Boolean))];
    return {
      silent,
      title: silent ? 'No audio was captured' : 'Your note could not be saved',
      message: silent ? 'The recording may have stopped before any sound reached leo. Check your microphone and shared audio, then try again.' : 'Your recording is kept so you can try recovering it. You can also delete it and start again.',
      details,
    };
  }

  function pastedScreenshot(clipboard) {
    const item = Array.from((clipboard && clipboard.items) || []).find((item) => item.kind === 'file' && /^image\/(png|jpeg|gif|webp)$/.test(item.type));
    return item ? item.getAsFile() : null;
  }

  async function saveScreenshot(view, file, encode, api, current) {
    if (!live(view)) return null;
    if (!file || !/^image\/(png|jpeg|gif|webp)$/.test(file.type) || file.size > 20 * 1024 * 1024) throw new Error('Paste a PNG, JPEG, GIF or WebP screenshot under 20 MB.');
    const id = view.id;
    const at = view.secs;
    const data = await encode(file);
    if (!live(current()) || current().id !== id) return null;
    const saved = await api(`/api/record/${id}/snapshot`, { method: 'POST', body: { data, at } });
    return live(current()) && current().id === id ? saved : null;
  }

  function create(deps) {
    const { api, esc, toast, go, noteHash, felix, icons, noteReady, encodePicture, noteFields, choiceCards, syncChoiceCards, bytes } = deps;
    const settingsSwitch = deps.settingsSwitch || ((attrs, on) => `<input type="checkbox" class="switch" role="switch" ${attrs}${on ? ' checked' : ''}>`);
    const s = {
      view: null,
      dismissed: (() => { try { return root.sessionStorage.getItem('leo-record-dismissed'); } catch (e) { return null; } })(),
      overview: null,
      config: null,
      calendar: null,
      event: null,
      eventChosen: false,
      wave: [],
      queue: [],
      smooth: 0,
      lastStep: 0,
      lastFed: 0,
      fedUpTo: 0,
      raf: 0,
      heard: '',
      showHeard: (() => {
        try {
          return root.localStorage.getItem('leo-rec-heard') !== 'shut';
        } catch (e) {
          return true;
        }
      })(),
      earlier: [],
      wantsDraft: null,
      wantsState: '',
      wantsTimer: 0,
      poll: 0,
      container: null,
      follow: true,
    };

    const desktopNow = () => (deps.desktop && deps.desktop()) || null;
    const micDenied = (source) => source !== 'screen' && desktopNow()?.permissions?.microphone === 'denied';

    const shown = (message) => Object.assign(new Error(message), { shown: true });


    function note(text) {
      if (!s.view) return;
      if (!s.view.warnings.includes(text)) s.view.warnings.push(text);
      draw();
    }

    async function pollNow() {
      if (!s.view) return;
      const id = s.view.id;
      try {
        const view = await api(`/api/record/${id}`);
        if (!s.view || s.view.id !== id || s.dismissed === id) return;
        accept(view);
      } catch (e) {
        if (!s.view || s.view.id !== id) return;
        if (e.status === 404) {
          s.view = null;
          draw();
          return;
        }
      }
      schedule();
    }

    const onScreen = () => root.document.visibilityState !== 'hidden' && root.__LEO_VISIBLE__ !== false;

    function schedule() {
      clearTimeout(s.poll);
      if (!onScreen()) return;
      if (s.view && (live(s.view) || s.view.state === 'writing')) s.poll = setTimeout(pollNow, POLL_EVERY);
    }

    function accept(view) {
      const before = s.view;
      const warnings = before && before.id === view.id ? before.warnings.filter((w) => !view.warnings.includes(w)) : [];
      s.earlier = before && before.id === view.id && s.earlier.length ? earlierFirst(s.earlier, placed(before)) : before && before.id === view.id ? s.earlier : [];
      s.view = { ...view, warnings: [...view.warnings, ...warnings] };
      if (view.state === 'failed' && !(before && before.state === 'failed')) api('/api/record').then((ov) => { s.overview = ov; draw(); }).catch(() => {});
      feedFromServer(view);
      if (view.state === 'done' && before && before.state !== 'done') {
        if (s.container && s.container.isConnected) go(noteHash(view.note), { replace: true });
        else if (noteReady) noteReady();
        if (view.writer_failed) toast('Saved the transcript. The notes were not written.', { action: 'Set up writing AI', run: () => go('#/settings/writing') });
        else toast('Your recording is now a note.', { action: 'Open', run: () => go(noteHash(view.note)) });
        root.leoMotion?.pulse(root.document.querySelector('.toast'));
      }
      draw();
      pill();
    }

    async function start(source, directory, title, profile) {
      const made = await api('/api/record', { method: 'POST', body: { source, directory, title, profile } });
      s.view = { id: made.id, source, state: 'starting', secs: 0, step: 'Starting', steps: null, transcript: '', warnings: [], points: [], points_total: 0, note: null, error: null };
      s.earlier = [];
      s.wantsDraft = null;
      s.wantsState = '';
      s.follow = true;
      Object.assign(s, { wave: [], queue: [], smooth: 0, lastStep: 0, lastFed: 0, fedUpTo: 0 });
      draw();
      pill();
      schedule();
    }

    async function setPaused(paused) {
      if (!s.view) return;
      accept(await api(`/api/record/${s.view.id}/pause`, { method: 'POST', body: { paused } }));
    }

    async function stop() {
      if (!s.view) return;
      const view = await api(`/api/record/${s.view.id}/stop`, { method: 'POST' });
      accept(view);
      schedule();
    }

    async function addPoint(text) {
      if (!s.view || !text.trim()) return;
      accept(await api(`/api/record/${s.view.id}/point`, { method: 'POST', body: { text } }));
    }

    function heardNow() {
      if (!s.view || s.view.state === 'paused') return 0;
      if (s.queue.length > MOST_LEVEL_QUEUE) s.queue.splice(0, s.queue.length - MOST_LEVEL_QUEUE);
      return s.queue.length ? s.queue.shift() : s.smooth * 0.85;
    }

    function feedFromServer(view) {
      if (!view || !Array.isArray(view.levels)) return;
      const { fresh, fedUpTo } = freshLevels(view.levels.map(loudness), view.levels_start || 0, s.fedUpTo);
      s.fedUpTo = fedUpTo;
      const per = Math.round(SERVER_STEP_MS / WAVE_STEP_MS);
      for (const level of fresh) {
        s.queue.push(...spread(s.lastFed, level, per));
        s.lastFed = level;
      }
      if (s.queue.length > MOST_LEVEL_QUEUE) s.queue.splice(0, s.queue.length - MOST_LEVEL_QUEUE);
    }

    function advance(now) {
      if (!s.lastStep || now - s.lastStep > 2000) s.lastStep = now;
      while (now - s.lastStep >= WAVE_STEP_MS) {
        s.smooth = ease(s.smooth, heardNow());
        s.wave.push(s.smooth);
        s.lastStep += WAVE_STEP_MS;
      }
      if (s.wave.length > MOST_WAVE) s.wave.splice(0, s.wave.length - MOST_WAVE);
    }

    function drawWave(now) {
      s.raf = 0;
      const canvas = root.document.getElementById('rec-wave');
      if (!canvas || !s.view || !live(s.view)) return;
      advance(now);
      const paused = s.view.state === 'paused';
      const ratio = root.devicePixelRatio || 1;
      const width = canvas.clientWidth;
      const height = canvas.clientHeight;
      if (canvas.width !== Math.round(width * ratio) || canvas.height !== Math.round(height * ratio)) {
        canvas.width = Math.round(width * ratio);
        canvas.height = Math.round(height * ratio);
      }
      const ctx = canvas.getContext('2d');
      ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
      ctx.clearRect(0, 0, width, height);
      const css = root.getComputedStyle(canvas);
      const on = css.getPropertyValue('--wave-on').trim() || '#d0342c';
      const off = css.getPropertyValue('--wave-off').trim() || '#c9c8c3';
      const pitch = BAR + GAP;
      const glide = paused ? 0 : Math.min(1, (now - s.lastStep) / WAVE_STEP_MS);
      const count = Math.ceil(width / pitch) + 2;
      for (let i = 0; i < count; i++) {
        const level = s.wave[s.wave.length - 1 - i] || 0;
        const x = width - BAR - (i + glide) * pitch;
        if (x < -BAR) break;
        const h = Math.max(3, level * (height - 6));
        ctx.fillStyle = paused || level < 0.06 ? off : on;
        ctx.beginPath();
        if (ctx.roundRect) ctx.roundRect(x, (height - h) / 2, BAR, h, BAR / 2);
        else ctx.rect(x, (height - h) / 2, BAR, h);
        ctx.fill();
      }
      const state = hearing(s.wave.slice(-200), WAVE_STEP_MS);
      const words = hearingWords(state, s.view.source, paused, micDenied(s.view.source));
      const label = root.document.getElementById('rec-hear');
      if (label && s.heard !== `${state}:${words}`) {
        s.heard = `${state}:${words}`;
        label.textContent = words;
        label.className = `rec-hear ${paused ? 'paused' : state}`;
      }
      if (onScreen()) s.raf = root.requestAnimationFrame(drawWave);
    }

    function paintWaveNow() {
      if (!root.requestAnimationFrame) return;
      if (s.raf && root.cancelAnimationFrame) root.cancelAnimationFrame(s.raf);
      s.raf = 0;
      drawWave(root.performance ? root.performance.now() : Date.now());
    }

    const KINDS = {
      microphone: { title: 'Microphone', about: 'A lecture, meeting or your voice' },
      screen: { title: 'Screen', about: 'Records what this computer plays' },
      call: { title: 'Call', about: 'You and others, on two tracks' },
    };
    const WAVE = [4, 9, 14, 7, 11, 16, 9, 5, 10, 13, 7, 4, 9, 12, 6];

    function track(icon, y) {
      const bars = WAVE.map((h, i) => `M${36 + i * 5} ${y - h / 2}v${h}`).join('');
      return `${icon.replace('<svg ', `<svg x="12" y="${y - 8}" width="16" height="16" `)}<path class="d-wave" d="${bars}"/>`;
    }

    function kindChoices(current = 'microphone') {
      const art = {
        microphone: track(icons.mic, 26),
        screen: track(icons.screen, 26),
        call: track(icons.mic, 16) + track(icons.screen, 36),
      };
      const choices = Object.keys(KINDS).map((kind) => [kind, KINDS[kind].title, KINDS[kind].about, `<svg viewBox="0 0 120 52">${art[kind]}</svg>`]);
      return choiceCards({ label: 'What to record', name: 'rec-kind', className: 'rec-kinds', current, choices });
    }

    function speakersOn() {
      try {
        const saved = root.localStorage.getItem('leo-label-speakers');
        if (saved !== null) return saved === 'on';
      } catch (e) {}
      return s.speakers === true;
    }

    function speakersReady() {
      const model = s.speakerModel;
      return Boolean(model && (model.verified || (model.progress && model.progress.state === 'downloading')));
    }

    function speakersRow() {
      const model = s.speakerModel;
      const job = model && model.progress;
      const getting = job && job.state === 'downloading';
      const missing = speakersOn() && model && !model.verified;
      const note = !missing ? '' : getting
        ? `Downloading the speaker model… ${Math.floor((100 * job.bytes) / Math.max(1, job.total))}%`
        : `${job && job.state === 'failed' ? 'The download failed. ' : ''}Labels need the speaker model (${bytes(model.size)}, stays on this computer).`;
      return `<label class="set-row rec-speakers"><span class="grow">Label speakers<span class="set-desc">For meetings: the transcript says who said what. Leave it off for lectures.</span></span>${settingsSwitch('id="rec-speakers"', speakersOn())}</label>`
        + (missing ? `<p class="set-note" id="rec-speakers-model">${esc(note)}${getting ? '' : ' <button type="button" class="btn plain sm" id="rec-speakers-get">Download</button>'}</p>` : '');
    }

    async function speakerModel(box) {
      const view = await api('/api/speech').catch(() => null);
      s.speakerModel = view && view.speakers ? view.speakers : null;
      redrawSpeakers(box);
      const job = s.speakerModel && s.speakerModel.progress;
      clearTimeout(s.speakerPoll);
      if (job && job.state === 'downloading') s.speakerPoll = setTimeout(() => { if (onScreen() && box.isConnected && box.querySelector('#rec-speakers-box')) speakerModel(box); }, 1000);
    }

    function redrawSpeakers(box) {
      const holder = box.querySelector('#rec-speakers-box');
      if (!holder) return;
      const html = speakersRow();
      if (holder.dataset.drawn === html) return;
      const focused = holder.contains(root.document.activeElement) ? root.document.activeElement.id : '';
      holder.dataset.drawn = html;
      holder.innerHTML = html;
      const toggle = holder.querySelector('#rec-speakers');
      toggle.addEventListener('change', () => {
        s.speakers = toggle.checked;
        try {
          root.localStorage.setItem('leo-label-speakers', toggle.checked ? 'on' : 'off');
        } catch (e) {}
        redrawSpeakers(box);
        const again = holder.querySelector('#rec-speakers');
        if (again) again.focus();
      });
      const get = holder.querySelector('#rec-speakers-get');
      if (get) {
        get.addEventListener('click', async () => {
          get.disabled = true;
          await api('/api/speech', { method: 'POST', body: { action: 'download', id: s.speakerModel.id } }).catch((e) => toast(e.message || String(e)));
          speakerModel(box);
        });
      }
      const back = focused && holder.querySelector(`#${focused}`);
      if (back) back.focus();
    }

    function drawIdle(folders, here, kind) {
      const ov = s.overview || {};
      const upcoming = (s.calendar && s.calendar.events) || [];
      const picked = s.event !== null ? upcoming[s.event] : null;
      const pending = ov.pending || [];
      const event = upcoming.length
        ? `<div class="field">${icons.calendar}<span class="select-control"><select id="rec-event" aria-label="Calendar event"><option value="">No calendar event</option>${upcoming.map((e, i) => `<option value="${i}"${i === s.event ? ' selected' : ''}>${esc(whenOf(e))} · ${esc(e.title)}</option>`).join('')}</select>${icons.chevron}</span></div>
          ${picked ? '<p class="hint rec-event-note">The AI gets this event’s description, place and who was invited.</p>' : ''}`
        : '';
      const connect = s.calendar && !s.calendar.connected ? '<p class="hint rec-calendar"><a href="#/settings/calendar">Connect a calendar</a> to name recordings after your classes or meetings.</p>' : '';
      const gaps = preflight(ov.checks).map((g) => `<p class="set-note">${esc(g.text)} <a href="${g.href}">${esc(g.link)}</a></p>`).join('')
        + (micDenied('microphone') ? '<p class="set-note">leo can’t hear: microphone access is off. <button class="btn plain sm" data-action="app-command" data-app-action="microphone">' + (desktopNow()?.platform === 'windows' ? 'Open Settings' : 'Open System Settings') + '</button></p>' : '')
        + (ov.checks && ov.checks.headset ? `<p class="set-note" id="rec-headset" hidden>${esc(HEADSET)}</p>` : '');
      const recover = pending.length
        ? `<div class="rec-recover">${pending.map((p) => `<div class="rec-recover-item"><div class="rec-recover-row">${icons.restore}<span class="grow">${esc(p.title || 'Interrupted recording')}<span class="sub">${esc(pendingLine(p))}</span></span></div><button type="button" class="btn plain sm" data-action="rec-recover" data-id="${esc(p.id)}">Recover</button><button type="button" class="btn plain sm" data-action="rec-discard" data-id="${esc(p.id)}" aria-label="Delete ${esc(p.title || 'interrupted recording')}">Delete</button></div>`).join('')}${ov.more ? '<button type="button" class="btn plain sm" data-action="rec-older">Show older</button>' : ''}</div>`
        : '';
      return `<div class="rec rec-idle">
        <div class="section-title">Record</div>
        ${recover ? `<section class="set-card"><header><h3>Not saved yet</h3></header>${recover}</section>` : ''}
        <section class="set-card rec-card">
          ${kindChoices(kind)}
          ${noteFields({ id: 'rec', folders, here, most: 4000, before: event })}
          <div id="rec-speakers-box">${speakersRow()}</div>
          ${connect}
          ${gaps}
          <div class="rec-start">
            <button class="rec-go" data-action="rec-start" aria-label="Start recording" ${ov.available === false ? 'disabled' : ''}><span class="rec-go-dot"></span></button>
            <span class="rec-start-label">Start recording</span>
            <p class="hint rec-tip">${esc(tipWords(desktopNow()?.platform))}</p>
          </div>
        </section>
      </div>`;
    }

    function drawLive() {
      const v = s.view;
      const paused = v.state === 'paused';
      const transcript = v.transcript ? esc(v.transcript) : `<span class="hint">${paused ? 'Paused.' : 'Listening… words appear here after a few seconds.'}</span>`;
      const listed = earlierFirst(s.earlier, placed(v));
      const more = listed.length && listed[0][0] > 0 ? '<button type="button" class="btn plain sm" data-action="rec-earlier">Show earlier points</button>' : '';
      const points = listed.length ? `${more}<ul class="rec-points">${listed.map(([, at, text]) => `<li><span class="rec-at">${clock(at)}</span><span class="rec-point-said">${esc(text)}</span></li>`).join('')}</ul>` : '';
      const warnings = v.warnings.map((w) => `<p class="set-note warn">${esc(w)}</p>`).join('');
      const kind = v.source === 'nativecall' ? 'Call · You + Others' : v.source === 'microphone' ? 'Microphone' : 'Screen';
      const controls = `<div class="rec-buttons">
            <button class="rec-round plain" data-action="rec-pause" aria-label="${paused ? 'Resume' : 'Pause'}">${paused ? '<svg viewBox="0 0 24 24"><path d="M8 5l12 7-12 7z" fill="currentColor"/></svg>' : '<svg viewBox="0 0 24 24"><rect x="6" y="5" width="4" height="14" rx="1" fill="currentColor"/><rect x="14" y="5" width="4" height="14" rx="1" fill="currentColor"/></svg>'}<span>${paused ? 'Resume' : 'Pause'}</span></button>
            <button class="rec-round stop rec-stop" data-action="rec-stop" aria-label="Stop and save"><span class="rec-square"></span><span>Stop and save</span></button>
          </div>`;
      return `<div class="rec rec-live${paused ? ' paused' : ''}">
        <section class="set-card rec-live-card">
          <div class="rec-live-top"><span class="rec-state"><span class="rec-dot"></span><span id="rec-word">${esc(stateWord(v))}</span></span><span class="rec-meta">${kind}</span></div>
          <div class="rec-clock"><span id="rec-time">${clock(v.secs)}</span></div>
          <div class="rec-wave-box"><canvas class="rec-wave" id="rec-wave" aria-hidden="true"></canvas></div><p class="rec-hear" id="rec-hear" role="status">${esc(hearingWords(hearing(s.wave, WAVE_STEP_MS), v.source, paused, micDenied(v.source)))}</p>${micDenied(v.source) ? `<button class="btn plain sm" data-action="app-command" data-app-action="microphone">${(desktopNow()?.platform === 'windows' ? 'Open Settings' : 'Open System Settings')}</button>` : ''}
          ${controls}
          ${warnings}
        </section>
        <details class="set-card rec-heard" id="rec-heard"${s.showHeard ? ' open' : ''}>
          <summary><span class="grow"><b>What’s being heard</b><span class="cal-sub">The live transcript</span></span>${icons.chevron}</summary>
          <div class="rec-transcript" id="rec-transcript" aria-live="polite">${transcript}</div>
        </details>
        <section class="set-card rec-notepad">
          <header><h3>Your notes</h3></header><p class="hint">Type points or paste screenshots; they go into the note as written and leo fills in the rest.</p>
          ${points}
          <div class="rec-point rec-compose"><textarea id="rec-point-text" class="set-input" rows="3" placeholder="Type a point and press Enter"></textarea><div class="rec-compose-foot"><span class="cal-sub">Shift + Enter starts a new line</span><button class="btn plain sm" data-action="rec-point">Add point</button></div></div>
        </section>
        <section class="set-card rec-wants-card">
          <header><h3>What you want from the notes</h3><span class="rec-wants-state" id="rec-wants-state">${esc(s.wantsState)}</span></header>
          <p class="hint">The AI follows this when it writes the note. You can change it until you stop.</p>
          <textarea id="rec-wants-live" class="set-input rec-wants" rows="2" maxlength="4000" placeholder="For example: focus on what will be on the exam, keep it short">${esc(s.wantsDraft !== null ? s.wantsDraft : v.wants || '')}</textarea>
        </section>
      </div>`;
    }

    function drawWriting() {
      const v = s.view;
      const now = Date.now();
      const step = v.steps ? `${v.steps[0]}/${v.steps[1]}` : String(v.step || '');
      const at = s.writeClock && s.writeClock.id === v.id ? s.writeClock : (s.writeClock = { id: v.id, since: now, step, stepSince: now, took: [] });
      if (at.step !== step) {
        at.took = [...at.took, (now - at.stepSince) / 1000].slice(-6);
        at.step = step;
        at.stepSince = now;
      }
      const typical = at.took.length ? at.took.reduce((a, b) => a + b, 0) / at.took.length : 20;
      const creep = 1 - Math.exp(-((now - at.stepSince) / 1000) / Math.max(3, typical));
      const done = v.steps ? Math.min(v.steps[0], v.steps[1]) : 0;
      const total = v.steps ? Math.max(1, v.steps[1]) : 0;
      const bar = Math.min(99, total ? ((done + (done < total ? 0.9 * creep : 0)) / total) * 100 : 60 * creep);
      const waited = Math.round((now - at.since) / 1000);
      return `<div class="rec rec-writing">
        <div class="section-title">Record</div>
        <section class="set-card rec-done-card">${felix.felix(72, 'idle think')}<h3>${esc(stateWord(v) || 'Writing the notes')}</h3>
          <div class="upload-bar" role="progressbar" aria-valuemin="0" aria-valuemax="100" aria-valuenow="${Math.round(Math.max(6, bar))}"><i style="width:${Math.max(6, bar)}%"></i></div>
          <p class="hint">Writing for ${clock(waited)} · ${clock(v.secs)} recorded. You can leave this page; the note appears in its folder when it is ready.</p>
          ${v.warnings.map((w) => `<p class="set-note warn">${esc(w)}</p>`).join('')}
          <button class="btn plain sm" data-action="rec-finish">Finish with what is transcribed so far</button>
        </section>
      </div>`;
    }

    function drawFailed() {
      const v = s.view;
      const failure = failureOf(v);
      const recoverable = !failure.silent && (s.overview?.pending || []).some((p) => p.id === v.session);
      return `<div class="rec rec-writing">
        <div class="section-title">Record</div>
        <section class="set-card rec-done-card rec-failed-card">${felix.felix(64, 'droop')}<h3>${esc(failure.title)}</h3>
          <p class="hint">${esc(failure.message)}</p>
          <div class="rec-failed-context">${icons.mic}<span>${esc(clock(v.secs))} recorded · ${v.source === 'nativecall' ? 'Call' : 'Recording'}</span></div>
          ${failure.details.length ? `<details class="rec-error-details source-more"><summary>${icons.chevron}<span>Error details</span></summary>${failure.details.map((w) => `<p>${esc(w)}</p>`).join('')}</details>` : ''}
          <div class="buttons">${recoverable ? `<button class="btn primary sm" data-action="rec-recover" data-id="${esc(v.session)}">Recover recording</button><button class="btn plain sm" data-action="rec-again">Record again</button>` : '<button class="btn primary sm" data-action="rec-again">Record again</button>'}<button class="btn sm plain danger-text" data-action="rec-discard" data-id="${esc(v.id)}">Delete recording</button><button class="btn plain sm" data-action="rec-dismiss">Close</button></div>
        </section>
      </div>`;
    }

    let folders = [];
    let here = '';

    const IDLE_FIELDS = ['#rec-dir', '#rec-title', '#rec-wants'];

    function chooseEvent(box, index, fill = true) {
      const events = (s.calendar && s.calendar.events) || [];
      const before = s.event !== null ? events[s.event] : null;
      const e = index !== null ? events[index] : null;
      s.event = e ? index : null;
      const title = box.querySelector('#rec-title');
      const wants = box.querySelector('#rec-wants');
      if (title && (!title.value.trim() || (before && title.value === before.title))) title.value = e ? e.title : '';
      if (wants && e && fill && !wants.value.trim()) wants.value = MEETING_WANTS;
      else if (wants && !e && wants.value === MEETING_WANTS) wants.value = '';
    }

    function pickKind(box, fill) {
      const picked = box.querySelector('input[name="rec-kind"]:checked');
      const call = Boolean(picked) && picked.value === 'call';
      const headset = box.querySelector('#rec-headset');
      if (headset) headset.hidden = !picked || picked.value === 'microphone';
      const wants = box.querySelector('#rec-wants');
      if (fill && call && wants && !wants.value.trim()) wants.value = MEETING_WANTS;
      const speakers = box.querySelector('#rec-speakers-box');
      if (speakers) speakers.hidden = call;
      const group = box.querySelector('.rec-kinds');
      if (group) syncChoiceCards(group);
    }

    function preselect(box, asked) {
      if (!box || !box.querySelector('.rec-idle')) return;
      const events = (s.calendar && s.calendar.events) || [];
      const wanted = asked.event ? events.findIndex((e) => e.id === asked.event) : -1;
      const at = wanted >= 0 ? wanted : s.eventChosen ? -1 : eventNow(events);
      if (wanted >= 0) s.eventChosen = true;
      if (at >= 0) chooseEvent(box, at, asked.kind !== 'microphone');
      const kind = ['microphone', 'screen', 'call'].includes(asked.kind) && box.querySelector(`input[name="rec-kind"][value="${asked.kind}"]`);
      if (kind && !kind.disabled) {
        kind.checked = true;
        pickKind(box, true);
      }
    }

    function idleForm(box) {
      if (!box.querySelector('.rec-idle')) return null;
      const picked = box.querySelector('input[name="rec-kind"]:checked');
      const active = root.document.activeElement;
      return {
        kind: picked ? picked.value : null,
        values: IDLE_FIELDS.map((at) => { const field = box.querySelector(at); return field ? field.value : null; }),
        focused: active && box.contains(active) && active.id ? { id: active.id, start: active.selectionStart, end: active.selectionEnd } : null,
      };
    }

    function restoreIdleForm(box, form) {
      if (!box.querySelector('.rec-idle')) return;
      const kind = form.kind && box.querySelector(`input[name="rec-kind"][value="${form.kind}"]`);
      if (kind && !kind.disabled) kind.checked = true;
      IDLE_FIELDS.forEach((at, i) => {
        const field = box.querySelector(at);
        const value = form.values[i];
        if (!field || value === null) return;
        if (field.tagName === 'SELECT' && ![...field.options].some((o) => o.value === value)) return;
        field.value = value;
      });
      const field = form.focused && root.document.getElementById(form.focused.id);
      if (field && box.contains(field)) {
        field.focus({ preventScroll: true });
        if (typeof form.focused.start === 'number' && typeof field.setSelectionRange === 'function') field.setSelectionRange(form.focused.start, form.focused.end);
      }
    }

    function draw() {
      const box = s.container;
      if (!box || !box.isConnected) return;
      const v = s.view;
      const typed = box.querySelector('#rec-point-text');
      const keep = typed ? { value: typed.value, focused: root.document.activeElement === typed, start: typed.selectionStart, end: typed.selectionEnd } : null;
      const wantsBox = box.querySelector('#rec-wants-live');
      const wantsKeep = wantsBox && root.document.activeElement === wantsBox ? { start: wantsBox.selectionStart, end: wantsBox.selectionEnd } : null;
      const old = box.querySelector('#rec-transcript');
      if (old) s.follow = old.scrollTop + old.clientHeight >= old.scrollHeight - 24;
      const form = idleForm(box);
      const motion = root.leoMotion;
      const page = !v || v.state === 'done' ? 'idle' : live(v) ? 'live' : v.state === 'writing' ? 'writing' : 'failed';
      const moved = s.page && s.page !== page;
      const paused = page === 'live' && s.paused !== undefined && s.paused !== (v.state === 'paused');
      if (moved) motion?.overlapOut(box);
      s.page = page;
      s.paused = page === 'live' ? v.state === 'paused' : undefined;
      const paint = () => {
        if (page === 'idle') box.innerHTML = drawIdle(folders, here, form && form.kind);
        else if (page === 'live') {
          box.innerHTML = drawLive();
          s.heard = '';
          paintWaveNow();
        }
        else if (page === 'writing') box.innerHTML = drawWriting();
        else box.innerHTML = drawFailed();
      };
      if (paused && motion) motion.iconSwap(box, paint, '.rec-round.plain svg');
      else paint();
      if (moved) motion?.cascade(box, 'settling');
      const heard = box.querySelector('#rec-heard');
      if (heard) {
        heard.addEventListener('toggle', () => {
          s.showHeard = heard.open;
          try {
            root.localStorage.setItem('leo-rec-heard', heard.open ? 'open' : 'shut');
          } catch (e) {}
          const text = heard.querySelector('#rec-transcript');
          if (heard.open && text) text.scrollTop = text.scrollHeight;
        });
      }
      const wantsArea = box.querySelector('#rec-wants-live');
      if (wantsArea) {
        if (wantsKeep) {
          wantsArea.focus();
          wantsArea.setSelectionRange(wantsKeep.start, wantsKeep.end);
        }
        wantsArea.addEventListener('input', () => {
          s.wantsDraft = wantsArea.value;
          s.wantsState = '';
          clearTimeout(s.wantsTimer);
          s.wantsTimer = setTimeout(saveWants, WANTS_SAVE_AFTER);
        });
        wantsArea.addEventListener('blur', () => {
          if (s.wantsDraft === null) return;
          clearTimeout(s.wantsTimer);
          saveWants();
        });
      }
      const area = box.querySelector('#rec-point-text');
      if (area && keep) {
        area.value = keep.value;
        if (keep.focused) {
          area.focus();
          area.setSelectionRange(keep.start, keep.end);
        }
      }
      if (area) {
        area.addEventListener('keydown', (e) => {
          if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) {
            e.preventDefault();
            submitPoint();
          }
        });
      }
      if (form) restoreIdleForm(box, form);
      redrawSpeakers(box);
      if (box.querySelector('#rec-speakers-box')) speakerModel(box);
      pickKind(box, false);
      box.querySelectorAll('input[name="rec-kind"]').forEach((radio) => radio.addEventListener('change', () => pickKind(box, true)));
      const event=box.querySelector('#rec-event');
      if (event) {
        event.addEventListener('change', () => {
          s.eventChosen = true;
          chooseEvent(box, event.value === '' ? null : Number(event.value));
          draw();
        });
      }
      const dir=box.querySelector('#rec-dir');
      const text = box.querySelector('#rec-transcript');
      if (text && s.follow) text.scrollTop = text.scrollHeight;
    }

    async function saveWants() {
      if (s.wantsDraft === null || !s.view || !live(s.view)) return;
      const text = s.wantsDraft;
      const id = s.view.id;
      try {
        const view = await api(`/api/record/${id}/wants`, { method: 'POST', body: { text } });
        if (!s.view || s.view.id !== id) return;
        if (s.wantsDraft === text) s.wantsDraft = null;
        s.wantsState = 'Saved';
        accept(view);
        const el = s.container && s.container.querySelector('#rec-wants-state');
        if (el) { el.textContent = 'Saved'; root.leoMotion?.pulse?.(el); }
        clearTimeout(s.wantsSavedTimer);
        s.wantsSavedTimer = setTimeout(() => {
          s.wantsState = '';
          const stale = s.container && s.container.querySelector('#rec-wants-state');
          if (stale) stale.textContent = '';
        }, 2000);
      } catch (e) {
        s.wantsState = '';
        toast(e.message, { bad: true });
      }
    }

    function submitPoint() {
      const area = s.container && s.container.querySelector('#rec-point-text');
      if (!area || !area.value.trim()) return;
      const text = area.value;
      area.value = '';
      addPoint(text).catch((e) => {
        if (e.status !== 503) area.value = text;
        toast(e.message, { bad: true });
      });
    }


    function pill() {
      let el = root.document.getElementById('rec-pill');
      const show = s.view && live(s.view) && !(s.container && s.container.isConnected);
      if (!show) {
        if (el) el.remove();
        return;
      }
      if (!el) {
        el = root.document.createElement('button');
        el.id = 'rec-pill';
        el.className = 'rec-pill';
        el.dataset.action = 'record';
        root.document.body.appendChild(el);
      }
      el.classList.toggle('paused', s.view.state === 'paused');
      el.innerHTML = `<span class="rec-dot"></span>${s.view.state === 'paused' ? 'Paused' : 'Recording'} ${clock(s.view.secs)}`;
    }

    async function refresh() {
      s.overview = await api('/api/record');
      const job = s.overview.job;
      if (job && job.id !== s.dismissed && job.state !== 'done') {
        accept(job);
        schedule();
      } else if (s.view && !live(s.view) && s.view.state !== 'writing') {
        s.view = null;
      }
    }

    let pasting = false;
    root.document.addEventListener('paste', (event) => {
      if (!s.container || !s.container.isConnected || !live(s.view) || !(s.container.contains(event.target) || event.target === root.document.body)) return;
      const file = pastedScreenshot(event.clipboardData);
      if (!file) return;
      event.preventDefault();
      if (pasting) return toast('A screenshot is still being added. Try again in a moment.');
      pasting = true;
      saveScreenshot(s.view, file, encodePicture, api, () => s.view).then((view) => {
        if (view) { accept(view); toast('Screenshot added to this recording.'); }
      }).catch((error) => toast(error.message || 'The screenshot could not be added. Try pasting it again.', { bad: true }))
        .finally(() => { pasting = false; });
    });

    function recordingVisibility() {
      clearTimeout(s.poll);
      if (!onScreen()) return;
      pollNow();
      paintWaveNow();
      if (s.speakerModel?.progress?.state === 'downloading' && s.container?.isConnected && s.container.querySelector('#rec-speakers-box')) speakerModel(s.container);
    }
    root.document.addEventListener('visibilitychange', recordingVisibility);
    root.addEventListener('leo-visibility', recordingVisibility);

    return {
      async show(container, options) {
        s.container = container;
        folders = options.folders || [];
        here = options.dir || '';
        draw();
        pill();
        await refresh();
        s.calendar = await api('/api/calendar').catch(() => null);
        preselect(s.container, options);
        draw();
        pill();
      },
      leave() {
        s.container = null;
        pill();
      },
      async begin() {
        const box = s.container;
        const picked = box.querySelector('input[name="rec-kind"]:checked');
        const kind = picked ? picked.value : 'microphone';
        const source = sourceFor(kind);
        const button = box.querySelector('[data-action="rec-start"]');
        if (button) button.disabled = true;
        try {
          const dir=box.querySelector('#rec-dir').value;
          const events = (s.calendar && s.calendar.events) || [];
          const picked = s.event !== null ? events[s.event] : null;
          await start(source, dir, box.querySelector('#rec-title').value.trim(), { context: picked ? picked.context : '', wants: box.querySelector('#rec-wants').value.trim(), speakers: kind !== 'call' && speakersOn() && speakersReady() });
        } finally {
          if (button && button.isConnected) button.disabled = false;
        }
      },
      pause: () => setPaused(!(s.view && s.view.state === 'paused')),
      stop,
      async finishAvailable() { if (s.view) { await api(`/api/record/${s.view.id}/finish`,{method:'POST'}); note('Queued transcription is skipped; any unfinished audio is kept for retry.'); } },
      async recover(id) {
        await api(`/api/record/${id}/recover`,{method:'POST'}); await refresh();
      },
      async older() {
        if (!s.overview?.more) return;
        const page = await api('/api/record?after=' + encodeURIComponent(s.overview.after));
        s.overview = mergePages(s.overview, page);
        draw();
      },

      async earlier() {
        if (!s.view) return;
        const listed = earlierFirst(s.earlier, placed(s.view));
        if (!listed.length || listed[0][0] <= 0) return;
        const id = s.view.id;
        const page = await api(`/api/record/${id}/points?before=${listed[0][0]}`);
        if (!s.view || s.view.id !== id) return;
        s.earlier = earlierFirst(page.points.map(([at, text], k) => [page.from + k, at, text]), s.earlier);
        draw();
      },
      point: submitPoint,
      dismiss() {
        if (!s.view || s.view.state !== 'failed') return;
        s.dismissed = s.view.id;
        try { root.sessionStorage.setItem('leo-record-dismissed', s.dismissed); } catch (e) {}
        clearTimeout(s.poll);
        s.view = null;
        draw();
        pill();
      },
      async discard(id) {
        const waiting = !s.view && (s.overview?.pending || []).some((p) => p.id === id);
        if (!waiting && (!s.view || s.view.id !== id || s.view.state !== 'failed')) throw shown('This recording is no longer on the error page. Refresh and try again.');
        clearTimeout(s.poll);
        await api(`/api/record/${id}`, { method: 'DELETE' });
        if (!waiting) {
          s.dismissed = id;
          try { root.sessionStorage.setItem('leo-record-dismissed', id); } catch (e) {}
          s.view = null;
        }
        await refresh();
        draw();
        pill();
        toast('Recording deleted');
      },
      again() {
        s.view = null;
        draw();
      },
      active: () => live(s.view),
    };
  }

  root.leoRecording = { pastedScreenshot, saveScreenshot, eventNow, whenOf, MEETING_WANTS, ease, spread, freshLevels, sourceFor, create, clock, stateWord, failureOf, pendingLine, mergePages, earlierFirst, unsavedLine, loudness, hearing, hearingWords, preflight, tipWords, HEADSET, QUIET };
})(typeof window !== 'undefined' ? window : globalThis);
