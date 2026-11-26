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
      c.speech === false && { name: 'Speech model', text: 'Needed to turn audio into text. The audio is kept until you choose one.', href: '#/settings/models', link: 'Choose model' },
      c.writing === false && { name: 'Writing AI', text: 'Without one you get a transcript, not notes.', href: '#/settings/writing', link: 'Set up' },
    ].filter(Boolean);
  }

  const tipWords = (platform) => `Keeps recording if you close the window. Open leo from the ${platform === 'windows' ? 'taskbar' : 'Dock'} to return.`;

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

  function tailMerge(shown, next) {
    const probe = next.slice(0, 64);
    const found = next.startsWith(shown.slice(0, probe.length)) ? 0 : shown.indexOf(probe);
    const drop = found > 0 ? found : 0;
    const rest = shown.slice(drop);
    const most = Math.min(rest.length, next.length);
    let keep = 0;
    while (keep < most && rest.charCodeAt(keep) === next.charCodeAt(keep)) keep++;
    if (/\S/.test(next[keep] || '')) keep -= /\S{0,32}$/.exec(next.slice(Math.max(0, keep - 32), keep))[0].length;
    if (keep > 0 && /[\uD800-\uDBFF]/.test(next[keep - 1])) keep--;
    return { drop, keep, tail: next.slice(keep) };
  }

  const NOT_SPEAKERS = ['', 'Microphone', 'Computer audio'];
  const TONES = ['var(--hue-0)', 'var(--hue-1)', 'var(--hue-4)', 'var(--hue-2)', 'var(--hue-5)', 'var(--hue-3)', 'var(--hue-6)', 'var(--hue-7)'];

  const speakersOf = (speakers) => (speakers || []).filter((name) => !NOT_SPEAKERS.includes(name));

  function toneOf(speakers, name) {
    if (name === 'You') return 'var(--text)';
    const i = speakersOf(speakers).filter((n) => n !== 'You').indexOf(name);
    return i < 0 ? 'var(--faint)' : TONES[i % TONES.length];
  }

  function pausesOf(trace) {
    const pauses = [];
    let paused = null;
    for (const t of trace || []) {
      const secs = Number(t.detail);
      if (t.stage === 'paused' && Number.isFinite(secs)) paused = { at: secs, from: Date.parse(t.at) };
      else if (t.stage === 'resumed' && paused) {
        pauses.push({ at: paused.at, mins: Math.round((Date.parse(t.at) - paused.from) / 60000) || 0 });
        paused = null;
      }
    }
    return pauses;
  }

  function transcriptFlow(passages, points, speakers, pauses = []) {
    const meeting = speakersOf(speakers).length > 0;
    const said = [
      ...(passages || []).map((row) => ({ at: row[1].start_secs || 0, passage: row })),
      ...(points || []).map((row) => ({ at: row[1].at_secs || 0, point: row })),
    ];
    const between = (p) => said.some((r) => r.at < p.at) && said.some((r) => r.at >= p.at);
    const rows = [...said, ...pauses.filter(between).map((p) => ({ at: p.at, pause: p }))].sort((a, b) => a.at - b.at || Boolean(b.pause) - Boolean(a.pause) || Boolean(a.point) - Boolean(b.point));
    const groups = [];
    let group = null;
    const open = (at, speaker, key) => {
      group = { at, speaker, key, items: [] };
      groups.push(group);
    };
    for (const row of rows) {
      if (row.pause) {
        groups.push({ at: row.at, pause: row.pause });
        group = null;
        continue;
      }
      if (row.point) {
        if (!group || (!meeting && group.key !== Math.floor(row.at / 60))) open(row.at, '', meeting ? null : Math.floor(row.at / 60));
        group.items.push({ point: row.point });
        continue;
      }
      const p = row.passage[1];
      const echo = p.echo === true;
      const key = meeting ? p.speaker || '' : Math.floor(row.at / 60);
      if (!group || (!echo && group.key !== key)) open(row.at, meeting ? p.speaker || '' : '', key);
      const last = group.items[group.items.length - 1];
      if (echo && last && last.echo) last.echo.push(row.passage);
      else group.items.push(echo ? { echo: [row.passage], who: p.speaker || '' } : { passage: row.passage });
    }
    return { meeting, groups };
  }

  const MOST_ASK = 8000;
  const FELIX_ASKS = {
    before: ['What should I listen for?', 'What should I ask?'],
    live: ['What did I just miss?', 'Sum up so far', 'What should I ask next?'],
  };

  function askChips(stage, esc) {
    return FELIX_ASKS[stage].map((q) => `<button type="button" class="starter" data-rec-ask="${esc(q)}">${esc(q)}</button>`).join('');
  }

  function newest(text, most) {
    const tail = text.length > most ? text.slice(text.length - most) : text;
    return /^[\uDC00-\uDFFF]/.test(tail) ? tail.slice(1) : tail;
  }

  function liveAsk(points, transcript) {
    const typed = points.length ? `My points so far:\n${newest(points.map((p) => `- ${p}`).join('\n'), 2000)}\n\n` : '';
    return `${typed}The newest part of the transcript:\n${newest(transcript || '', MOST_ASK - typed.length - 40) || '(nothing heard yet)'}`;
  }

  function briefAsk(event, wants) {
    return [event && `Calendar event: ${event.title} (${whenOf(event)})${event.context ? `\n${event.context}` : ''}`, wants && `What I want from the notes: ${wants}`].filter(Boolean).join('\n\n');
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
    if (!file || !/^image\/(png|jpeg|gif|webp)$/.test(file.type) ) throw new Error('Paste a PNG, JPEG, GIF or WebP screenshot.');
    const id = view.id;
    const at = view.secs;
    const data = await encode(file);
    if (!live(current()) || current().id !== id) return null;
    const saved = await api(`/api/record/${id}/snapshot`, { method: 'POST', body: { data, at } });
    return live(current()) && current().id === id ? saved : null;
  }

  function create(deps) {
    const { api, esc, toast, go, noteHash, felix, icons, noteReady, encodePicture, choiceCards, syncChoiceCards, bytes } = deps;
    const folderOptions = deps.folderOptions || (() => '<option value="">Top level</option>');
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
      tab: 'points',
      liveEvent: null,
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

    async function start(source, directory, title, profile, event) {
      const made = await api('/api/record', { method: 'POST', body: { source, directory, title, profile } });
      s.view = { id: made.id, source, title, directory, state: 'starting', secs: 0, step: 'Starting', steps: null, transcript: '', warnings: [], points: [], points_total: 0, note: null, error: null };
      s.liveEvent = event ? { id: made.id, event } : null;
      s.tab = 'points';
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
    function kindChoices(current = 'microphone') {
      const choices = Object.keys(KINDS).map((kind) => [kind, KINDS[kind].title, '', '', kind === 'screen' ? icons.screen : icons.mic]);
      return choiceCards({ label: 'What to record', name: 'rec-kind', className: 'rec-kinds', current, choices }) + `<p class="hint rec-kind-line" id="rec-kind-line">${esc((KINDS[current] || KINDS.microphone).about)}</p>`;
    }

    function speakersOn() {
      const picked = s.container && s.container.querySelector('input[name="rec-kind"]:checked');
      return Boolean(picked) && picked.value === 'call';
    }

    function speakersReady() {
      const model = s.speakerModel;
      return Boolean(model && (model.verified || (model.progress && model.progress.state === 'downloading')));
    }

    function speakersRow() {
      const model = s.speakerModel;
      const job = model && model.progress;
      const getting = job && job.state === 'downloading';
      if (!speakersOn() || !model || model.verified) return '';
      const note = getting
        ? `Downloading the speaker model… ${Math.floor((100 * job.bytes) / Math.max(1, job.total))}%`
        : `${job && job.state === 'failed' ? 'The download failed. ' : ''}Calls say who spoke, which needs the speaker model (${bytes(model.size)}, stays on this computer).`;
      return `<p class="set-note" id="rec-speakers-model">${esc(note)}${getting ? '' : ' <button type="button" class="btn plain sm" id="rec-speakers-get">Download</button>'}</p>`;
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
      holder.dataset.drawn = html;
      holder.innerHTML = html;
      const get = holder.querySelector('#rec-speakers-get');
      if (get) {
        get.addEventListener('click', async () => {
          get.disabled = true;
          await api('/api/speech', { method: 'POST', body: { action: 'download', id: s.speakerModel.id } }).catch((e) => toast(e.message || String(e)));
          speakerModel(box);
        });
      }
    }

    function chip(icon, text) {
      return `<span class="chip">${icon}${esc(text)}</span>`;
    }

    function selectChip(icon, id, label, options) {
      return `<span class="chip rec-chip">${icon}<span class="select-control"><select id="${id}" aria-label="${label}">${options}</select>${icons.chevron}</span></span>`;
    }

    function drawIdle(folders, here, kind) {
      const ov = s.overview || {};
      const upcoming = (s.calendar && s.calendar.events) || [];
      const picked = s.event !== null ? upcoming[s.event] : null;
      const pending = ov.pending || [];
      const event = upcoming.length
        ? selectChip(icons.calendar, 'rec-event', 'Calendar event', `<option value="">No calendar event</option>${upcoming.map((e, i) => `<option value="${i}"${i === s.event ? ' selected' : ''}>${esc(whenOf(e))} · ${esc(e.title)}</option>`).join('')}`)
        : '';
      const folder = selectChip(icons.folder, 'rec-dir', 'Folder', folderOptions(folders, here));
      const missing = preflight(ov.checks);
      const calendarRow = s.calendar && !s.calendar.connected ? [{ name: 'Calendar', text: 'Optional: names recordings after your classes or meetings.', href: '#/settings/calendar', link: 'Connect', optional: true }] : [];
      const rows = [...missing, ...calendarRow];
      const ready = ov.checks && !missing.length ? '<p class="rec-ready"><span class="pencil-tick" aria-hidden="true"></span>Ready · the speech model and writing AI are set up</p>' : '';
      const connect = rows.length ? `<div class="rec-checks">${rows.map((g) => `<div class="rec-check${g.optional ? ' optional' : ''}"><span class="rec-check-dot" aria-hidden="true"></span><span class="rec-check-text"><b>${esc(g.name)}</b><span>${esc(g.text)}</span></span><a class="btn plain sm" href="${g.href}">${esc(g.link)}</a></div>`).join('')}</div>` : '';
      const gaps = (micDenied('microphone') ? '<p class="set-note">leo can’t hear: microphone access is off. <button class="btn plain sm" data-action="app-command" data-app-action="microphone">' + (desktopNow()?.platform === 'windows' ? 'Open Settings' : 'Open System Settings') + '</button></p>' : '')
        + (ov.checks && ov.checks.headset ? `<p class="set-note" id="rec-headset" hidden>${esc(HEADSET)}</p>` : '');
      const recover = pending.length
        ? `<div class="rec-recover">${pending.map((p) => `<div class="rec-recover-item"><div class="rec-recover-row">${icons.restore}<span class="grow">${esc(p.title || 'Interrupted recording')}<span class="sub">${esc(pendingLine(p))}</span></span></div><button type="button" class="btn plain sm" data-action="rec-recover" data-id="${esc(p.id)}">Recover</button><button type="button" class="btn plain sm" data-action="rec-discard" data-id="${esc(p.id)}" aria-label="Delete ${esc(p.title || 'interrupted recording')}">Delete</button></div>`).join('')}${ov.more ? '<button type="button" class="btn plain sm" data-action="rec-older">Show older</button>' : ''}</div>`
        : '';
      return `<div class="rec rec-idle">
        ${recover ? `<section class="set-card"><header><h3>Not saved yet</h3></header>${recover}</section>` : ''}
        <header class="rec-head note-head"><div class="rec-head-text">
          <input id="rec-title" class="rec-title" placeholder="Untitled recording" aria-label="Title (optional)" autocomplete="off" spellcheck="true">
          <div class="rec-chips">${event}${folder}</div>
          ${picked ? '<p class="hint rec-event-note">The AI gets this event’s description, place and who was invited.</p>' : ''}
        </div></header>
        <section class="set-card rec-card">
          ${kindChoices(kind)}
          <label class="field field-area"><textarea id="rec-wants" rows="2" data-reads="4000" aria-label="What you want from it (optional)" placeholder="What do you want from it? e.g. the main ideas, step by step"></textarea></label>
          <p class="set-desc" id="rec-wants-hint" hidden>Felix reads the first 4,000 characters of this.</p>
          <div id="rec-speakers-box">${speakersRow()}</div>
          ${ready}
          ${connect}
          ${gaps}
          <div class="rec-start">
            <button class="rec-go" data-action="rec-start" ${ov.available === false ? 'disabled' : ''}>Start recording</button>
            <p class="hint rec-tip">${esc(tipWords(desktopNow()?.platform))}</p>
            <div class="rec-ask" id="rec-ask" hidden><div class="chat-starters">${askChips('before', esc)}</div></div>
          </div>
        </section>
      </div>`;
    }

    const PAUSE_ICON = '<svg viewBox="0 0 24 24" aria-hidden="true"><rect x="6" y="5" width="4" height="14" rx="1" fill="currentColor"/><rect x="14" y="5" width="4" height="14" rx="1" fill="currentColor"/></svg>';
    const RESUME_ICON = '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M8 5l12 7-12 7z" fill="currentColor"/></svg>';

    function recBar(v, lead) {
      const word = v.state === 'paused' ? 'Resume' : 'Pause';
      return `<div class="rec-bar" role="group" aria-label="Recorder">${lead}<span class="rec-wave-box"><canvas class="rec-wave" id="rec-wave" aria-hidden="true"></canvas></span><button type="button" class="rec-pause" id="rec-pause" data-keep data-action="rec-pause" aria-label="${word}" title="${word}">${v.state === 'paused' ? RESUME_ICON : PAUSE_ICON}<span class="said">${word}</span></button><button type="button" class="rec-stop" id="rec-stop" data-keep data-action="rec-stop"><span class="rec-square"></span>Stop and save</button></div>`;
    }

    function liveTab(tab, text, panel, on, extra = '') {
      return `<button type="button" role="tab" class="kanban-tab${on === tab ? ' on' : ''}" id="rec-tab-${tab}" data-rec-tab="${tab}" aria-controls="${panel}" aria-selected="${on === tab}" tabindex="${on === tab ? 0 : -1}">${text}${extra}</button>`;
    }

    function drawLive() {
      const v = s.view;
      const writing = v.state === 'writing';
      const paused = v.state === 'paused';
      const listed = earlierFirst(s.earlier, placed(v));
      const tab = writing && s.tab === 'wants' ? 'points' : s.tab || 'points';
      const more = listed.length && listed[0][0] > 0 ? '<button type="button" class="btn plain sm" data-action="rec-earlier">Show earlier points</button>' : '';
      const points = listed.length ? `${more}<ul class="rec-points">${listed.map(([, at, text]) => `<li><span class="rec-at">${clock(at)}</span><span class="rec-point-said">${esc(text)}</span></li>`).join('')}</ul>` : '';
      const warnings = v.warnings.map((w) => `<p class="set-note warn">${esc(w)}</p>`).join('');
      const kind = v.source === 'nativecall' ? 'Call · You + Others' : v.source === 'microphone' ? 'Microphone' : 'Screen';
      const event = s.liveEvent && s.liveEvent.id === v.id ? s.liveEvent.event : null;
      const chips = [event ? chip(icons.calendar, whenOf(event)) : '', chip(icons.folder, v.directory || 'Top level'), chip(kind === 'Screen' ? icons.screen : icons.mic, kind)].join('');
      const denied = micDenied(v.source);
      const bar = writing ? '' : recBar(v, `<span class="rec-dot"></span><span class="said" id="rec-word">${esc(stateWord(v))}</span><span class="rec-time" id="rec-time">${clock(v.secs)}</span>`);
      const hear = writing ? '' : `<p class="rec-hear" id="rec-hear" role="status">${esc(hearingWords(hearing(s.wave, WAVE_STEP_MS), v.source, paused, denied))}</p>${denied ? `<button class="btn plain sm" data-action="app-command" data-app-action="microphone">${(desktopNow()?.platform === 'windows' ? 'Open Settings' : 'Open System Settings')}</button>` : ''}`;
      const wanted = s.wantsDraft !== null ? s.wantsDraft : v.wants || '';
      const count = v.points_total ?? listed.length;
      const tabs = `<div class="rec-tabs" id="rec-tabs" role="tablist" aria-label="This recording" data-keep>${writing ? '<button type="button" role="tab" class="kanban-tab" aria-disabled="true" aria-selected="false" disabled>Notes</button>' : ''}${liveTab('points', 'Your notes', 'rec-points-panel', tab, ` <span class="badge" id="rec-count">${count}</span>`)}${liveTab('heard', 'Transcript', 'rec-heard', tab)}${writing ? '' : liveTab('wants', 'What you want', 'rec-wants-panel', tab)}</div>`;
      const compose = writing ? '' : '<div class="rec-point rec-compose"><textarea id="rec-point-text" class="set-input" rows="3" placeholder="Type a point and press Enter"></textarea><div class="rec-compose-foot"><span class="cal-sub">Shift + Enter starts a new line</span><button class="btn plain sm" data-action="rec-point">Add point</button></div></div>';
      const wants = writing ? '' : `<section class="rec-panel rec-wants-card" id="rec-wants-panel" role="tabpanel" aria-labelledby="rec-tab-wants"${tab === 'wants' ? '' : ' hidden'}>
            <p class="hint">The AI follows this when it writes the note. You can change it until you stop. <span class="rec-wants-state" id="rec-wants-state">${esc(s.wantsState)}</span></p>
            <textarea id="rec-wants-live" class="set-input rec-wants" data-keep rows="3" data-reads="4000" aria-label="What you want from the notes" placeholder="For example: focus on what will be on the exam, keep it short">${esc(wanted)}</textarea>
            <p class="set-desc" id="rec-wants-live-hint"${Array.from(wanted.trim()).length > 4000 ? '' : ' hidden'}>Felix reads the first 4,000 characters of this.</p>
          </section>`;
      return `<div class="rec rec-live${paused ? ' paused' : ''}${writing ? ' writing' : ''}">
        <header class="rec-head note-head"><div class="rec-head-text"><h1>${esc(v.title || 'Untitled recording')}</h1><div class="rec-chips">${chips}</div>${hear}</div></header>
        ${writing ? drawWriting() : ''}
        <div class="rec-warnings">${warnings}</div>
        <div class="rec-body">
          <div class="rec-main">
            ${tabs}
            <section class="rec-panel rec-notepad" id="rec-points-panel" role="tabpanel" aria-labelledby="rec-tab-points"${tab === 'points' ? '' : ' hidden'}>
              ${writing ? '' : '<p class="hint">Type points or paste screenshots; they go into the note as written and leo fills in the rest.</p>'}
              ${points || (writing ? '<p class="hint">You typed no points.</p>' : '')}
              ${compose}
            </section>
            <section class="rec-panel rec-heard" id="rec-heard" role="tabpanel" data-keep aria-labelledby="rec-tab-heard">
              <div class="rec-transcript" id="rec-transcript" tabindex="-1" aria-live="off"></div>
              <button type="button" class="btn plain sm rec-latest" id="rec-latest" hidden>Back to latest</button>
            </section>
            ${wants}
          </div>
          ${writing ? '' : `<aside class="set-card rec-felix rec-ask" id="rec-ask" data-keep aria-label="Ask Felix"><header><h3>Felix</h3></header><div class="chat-starters">${askChips('live', esc)}</div><form class="rec-ask-own" data-rec-ask-form><input id="rec-ask-text" class="set-input" placeholder="Ask about the recording…" aria-label="Ask Felix about the recording" autocomplete="off" maxlength="2000"><button type="submit" class="icon-btn" aria-label="Ask">${icons.send}</button></form></aside>`}
        </div>
        ${bar ? `<div class="rec-bar-dock">${bar}</div>` : ''}
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
      const said = stateWord(v);
      const words = !said || said.startsWith('Writing the notes') ? said.replace(/^Writing the notes ?/, '') : said;
      return `<div class="rec-progress"><svg class="rec-mark" aria-hidden="true"><use href="#leo-mark"/></svg><span class="rec-progress-step">Writing the notes${words ? ` · ${esc(words)}` : ''}</span><div class="upload-bar" role="progressbar" aria-label="Writing the notes" aria-valuemin="0" aria-valuemax="100" aria-valuenow="${Math.round(Math.max(6, bar))}"><i style="width:${Math.max(6, bar)}%"></i></div><button type="button" class="btn plain sm" data-action="rec-finish" title="Finish with what is transcribed so far">Finish now</button></div>
        <p class="hint rec-progress-hint">Writing for ${clock(waited)} · ${clock(v.secs)} recorded. You can leave this page; the note appears in its folder when it is ready.</p>`;
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
      const group = box.querySelector('.rec-kinds');
      if (group) syncChoiceCards(group);
      const line = box.querySelector('#rec-kind-line');
      if (line && picked && KINDS[picked.value]) line.textContent = KINDS[picked.value].about;
      redrawSpeakers(box);
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

    function graft(now, made, kept) {
      const fresh = [...made.children];
      if (fresh.length !== now.children.length) return false;
      return [...now.children].every((child, i) => {
        if (kept.includes(child)) return fresh[i].id === child.id;
        if (!kept.some((k) => child.contains(k))) return child.replaceWith(fresh[i]) || true;
        child.className = fresh[i].className;
        return graft(child, fresh[i], kept);
      });
    }

    function paintLive(box) {
      const html = drawLive();
      const kept = [...box.querySelectorAll('[data-keep]')];
      const made = root.document.createElement('div');
      made.innerHTML = html;
      const next = { pause: made.querySelector('#rec-pause'), wants: made.querySelector('#rec-wants-live') };
      if (!kept.length || !graft(box, made, kept)) return void (box.innerHTML = html);
      const pause = box.querySelector('#rec-pause');
      if (pause && next.pause && pause.innerHTML !== next.pause.innerHTML) {
        pause.innerHTML = next.pause.innerHTML;
        ['aria-label', 'title'].forEach((name) => pause.setAttribute(name, next.pause.getAttribute(name)));
      }
      const wants = box.querySelector('#rec-wants-live');
      if (wants && next.wants && wants !== root.document.activeElement && wants.value !== next.wants.value) wants.value = next.wants.value;
    }

    function syncAsk(box) {
      const ask = box.querySelector('.rec-idle #rec-ask');
      const wants = box.querySelector('#rec-wants');
      if (ask) ask.hidden = s.event === null && !(wants && wants.value.trim());
    }

    function askFelix(question) {
      const chat = deps.chat && deps.chat();
      if (!chat) return;
      if (chat.isBusy()) return toast('Felix is still answering. Try again in a moment.', { bad: true });
      const v = s.view;
      const events = (s.calendar && s.calendar.events) || [];
      const wants = s.container && s.container.querySelector('#rec-wants');
      const text = live(v) ? liveAsk(earlierFirst(s.earlier, placed(v)).map(([, , said]) => said), v.transcript) : briefAsk(s.event !== null ? events[s.event] : null, wants ? wants.value.trim() : '');
      chat.attachSelection({ note: '', title: 'Live recording', text });
      chat.askNew(question);
      return true;
    }

    function showTranscript(box) {
      const text = box.querySelector('#rec-transcript');
      if (!text) return;
      const latest = box.querySelector('#rec-latest');
      const fresh = s.shownIn !== text;
      if (fresh) {
        Object.assign(s, { shownIn: text, shownFor: null, follow: true });
        text.addEventListener('scroll', () => {
          s.follow = text.scrollTop + text.clientHeight >= text.scrollHeight - 24;
          latest.hidden = s.follow;
        });
        latest.addEventListener('click', () => {
          s.follow = true;
          latest.hidden = true;
          text.focus({ preventScroll: true });
          text.scrollTop = text.scrollHeight;
        });
      }
      const turns = s.view.turns || [];
      const next = turns.length ? turns : s.view.transcript ? [{ n: 0, text: s.view.transcript }] : [];
      const anew = fresh || s.shownFor !== s.view.id;
      if (anew) Object.assign(s, { shownFor: s.view.id, parts: new Map() });
      const { parts } = s;
      text.className = `rec-transcript${turns.length ? ' transcript-flow' : ''}`;
      if (!next.length) {
        parts.clear();
        text.innerHTML = `<span class="hint">${s.view.state === 'paused' ? 'Paused.' : s.view.state === 'writing' ? 'Nothing was heard while recording.' : 'Listening… words appear here after a few seconds.'}</span>`;
        return;
      }
      const picked = root.getSelection && root.getSelection();
      if (!(picked && !picked.isCollapsed && picked.containsNode(text, true))) {
        text.querySelectorAll('.rec-new').forEach((span) => { if (!(span.getAnimations && span.getAnimations().length)) span.replaceWith(...span.childNodes); });
        text.normalize();
      }
      if (!parts.size) text.textContent = '';
      const height = text.scrollHeight;
      const keeps = new Set(next.map((part) => part.n));
      const forget = (n) => { parts.get(n).row.remove(); parts.delete(n); };
      [...parts.keys()].filter((n) => n < next[0].n).forEach(forget);
      const first = parts.get(next[0].n);
      if (first) dropWords(first, tailMerge(first.text, next[0].text).drop);
      const lost = height - text.scrollHeight;
      if (lost && !s.follow) text.scrollTop -= lost;
      [...parts.keys()].filter((n) => !keeps.has(n)).forEach(forget);
      let above = null;
      for (const { n, speaker, text: said } of next) {
        let part = parts.get(n);
        if (!part) {
          part = turns.length ? turnRow(speaker) : { row: text, words: text, text: '' };
          parts.set(n, part);
          if (above) above.after(part.row);
          else if (turns.length) text.prepend(part.row);
        }
        if (part.text !== said) setWords(part, said, anew);
        above = part.row;
      }
      if (s.follow) text.scrollTop = text.scrollHeight;
      latest.hidden = s.follow;
    }

    function turnRow(speaker) {
      const make = (tag, className, ...inside) => {
        const el = Object.assign(root.document.createElement(tag), { className });
        el.append(...inside);
        return el;
      };
      const dot = make('span', 'tone-dot');
      const order = s.toneOrder && s.toneOrder.id === (s.view && s.view.id) ? s.toneOrder : (s.toneOrder = { id: s.view && s.view.id, names: [] });
      if (!order.names.includes(speaker)) order.names.push(speaker);
      dot.style.setProperty('--tone', toneOf(order.names, speaker));
      const words = make('p', 'spoken');
      return { row: make('div', 'turn', make('div', 'turn-who', dot, make('span', 'turn-name', speaker)), words), words, text: '' };
    }

    function dropWords(part, left) {
      part.text = part.text.slice(left);
      while (left > 0 && part.words.firstChild) {
        const node = part.words.firstChild;
        const size = node.textContent.length;
        if (size > left) (node.firstChild || node).deleteData(0, left);
        else node.remove();
        left -= Math.min(size, left);
      }
    }

    function setWords(part, next, plain) {
      const { drop, keep, tail } = tailMerge(part.text, next);
      dropWords(part, drop);
      let size = part.text.length;
      while (size > keep && part.words.lastChild) {
        const node = part.words.lastChild;
        const length = node.textContent.length;
        if (size - length >= keep) node.remove();
        else (node.firstChild || node).deleteData(length - (size - keep), size - keep);
        size = Math.max(keep, size - length);
      }
      part.text = next;
      if (tail) part.words.append(plain ? root.document.createTextNode(tail) : Object.assign(root.document.createElement('span'), { className: 'rec-new', textContent: tail }));
    }

    function syncTabs(box) {
      const tabs = [...box.querySelectorAll('[data-rec-tab]')];
      if (!tabs.some((t) => t.dataset.recTab === s.tab)) s.tab = 'points';
      const count = box.querySelector('#rec-count');
      if (count && s.view) count.textContent = String(s.view.points_total ?? earlierFirst(s.earlier, placed(s.view)).length);
      for (const t of tabs) {
        const on = t.dataset.recTab === s.tab;
        t.classList.toggle('on', on);
        t.setAttribute('aria-selected', String(on));
        t.tabIndex = on ? 0 : -1;
        const panel = root.document.getElementById(t.getAttribute('aria-controls'));
        if (panel) panel.hidden = !on;
      }
    }

    function showTab(tab, focus) {
      const box = s.container;
      if (!box || !box.isConnected) return;
      s.tab = tab;
      syncTabs(box);
      const picked = box.querySelector(`[data-rec-tab="${tab}"]`);
      if (focus && picked) picked.focus();
      const text = box.querySelector('#rec-transcript');
      if (tab === 'heard' && text && s.follow) text.scrollTop = text.scrollHeight;
    }

    function draw() {
      const box = s.container;
      if (!box || !box.isConnected) return;
      const v = s.view;
      const typed = box.querySelector('#rec-point-text');
      const keep = typed ? { value: typed.value, focused: root.document.activeElement === typed, start: typed.selectionStart, end: typed.selectionEnd } : null;
      const wantsBox = box.querySelector('#rec-wants-live');
      const wantsKeep = wantsBox && root.document.activeElement === wantsBox ? { start: wantsBox.selectionStart, end: wantsBox.selectionEnd } : null;
      const form = idleForm(box);
      const motion = root.leoMotion;
      const page = !v || v.state === 'done' ? 'idle' : live(v) ? 'live' : v.state === 'writing' ? 'writing' : 'failed';
      const moved = s.page && s.page !== page;
      const paused = page === 'live' && s.paused !== undefined && s.paused !== (v.state === 'paused');
      const flowing = (s.page === 'live' && page === 'writing') || (s.page === 'writing' && page === 'live');
      if (moved && !flowing) motion?.overlapOut(box);
      s.page = page;
      s.paused = page === 'live' ? v.state === 'paused' : undefined;
      const paint = () => {
        if (page === 'idle') box.innerHTML = drawIdle(folders, here, form && form.kind);
        else if (page === 'live' || page === 'writing') {
          paintLive(box);
          s.heard = '';
          if (page === 'live') paintWaveNow();
        }
        else box.innerHTML = drawFailed();
      };
      if (paused && motion) motion.iconSwap(box, paint, '.rec-pause svg');
      else paint();
      if (moved && !flowing) motion?.cascade(box, 'settling');
      if (page === 'live' || page === 'writing') {
        syncTabs(box);
        showTranscript(box);
      }
      const wantsArea = box.querySelector('#rec-wants-live');
      if (wantsArea) {
        if (wantsKeep && root.document.activeElement !== wantsArea) {
          wantsArea.focus();
          wantsArea.setSelectionRange(wantsKeep.start, wantsKeep.end);
        }
      }
      if (wantsArea && !wantsArea.dataset.bound) {
        wantsArea.dataset.bound = '1';
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
      box.querySelectorAll('input[name="rec-kind"]').forEach((radio) => radio.addEventListener('change', () => { pickKind(box, true); syncAsk(box); }));
      syncAsk(box);
      const idleWants = box.querySelector('.rec-idle #rec-wants');
      if (idleWants) idleWants.addEventListener('input', () => syncAsk(box));
      const event=box.querySelector('#rec-event');
      if (event) {
        event.addEventListener('change', () => {
          s.eventChosen = true;
          chooseEvent(box, event.value === '' ? null : Number(event.value));
          draw();
        });
      }
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


    function floatBar(show) {
      let el = root.document.getElementById('rec-float');
      if (!show) {
        if (el) el.remove();
        return;
      }
      if (!el) {
        el = root.document.createElement('div');
        el.id = 'rec-float';
        el.className = 'rec-bar-dock rec-float';
        root.document.body.appendChild(el);
      }
      if (el.dataset.state !== s.view.state) {
        el.dataset.state = s.view.state;
        const focused = root.document.activeElement?.closest?.('#rec-float') ? root.document.activeElement.id : '';
        el.innerHTML = recBar(s.view, '<button type="button" class="rec-float-open" id="rec-float-open" data-action="record" title="Open the recording"><span class="rec-dot"></span><span class="rec-time" id="rec-time"></span></button>');
        if (focused) el.querySelector(`#${focused}`)?.focus();
      }
      el.classList.toggle('paused', s.view.state === 'paused');
      el.querySelector('#rec-time').textContent = clock(s.view.secs);
      if (!s.raf) paintWaveNow();
    }

    function pill() {
      floatBar(Boolean(s.view && live(s.view) && !(s.container && s.container.isConnected)));
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

    root.document.addEventListener('click', (event) => {
      const button = event.target && event.target.closest && event.target.closest('[data-rec-ask]');
      if (button && s.container && s.container.contains(button)) askFelix(button.dataset.recAsk);
      const tab = event.target && event.target.closest && event.target.closest('[data-rec-tab]');
      if (tab && s.container && s.container.contains(tab)) showTab(tab.dataset.recTab, false);
    });

    root.document.addEventListener('submit', (event) => {
      const form = event.target && event.target.closest && event.target.closest('[data-rec-ask-form]');
      if (!form || !s.container || !s.container.contains(form)) return;
      event.preventDefault();
      const input = form.querySelector('input');
      const question = input.value.trim();
      if (!question) return;
      if (askFelix(question)) input.value = '';
    });

    root.document.addEventListener('keydown', (event) => {
      const tab = event.target && event.target.closest && event.target.closest('[data-rec-tab]');
      if (!tab || !s.container || !s.container.contains(tab) || !['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
      const tabs = [...s.container.querySelectorAll('[data-rec-tab]')];
      const at = tabs.indexOf(tab);
      const next = event.key === 'Home' ? 0 : event.key === 'End' ? tabs.length - 1 : (at + (event.key === 'ArrowRight' ? 1 : -1) + tabs.length) % tabs.length;
      event.preventDefault();
      showTab(tabs[next].dataset.recTab, true);
    });

    let pasting = false;
    root.document.addEventListener('paste', (event) => {
      if (!s.container || !s.container.isConnected || !live(s.view) || !(s.container.contains(event.target) || event.target === root.document.body)) return;
      const file = pastedScreenshot(event.clipboardData);
      if (!file || root.leoEditing?.leoCopied(event.clipboardData.getData('text/html'), event.clipboardData.getData('text/plain'))?.alone === false) return;
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
          await start(source, dir, box.querySelector('#rec-title').value.trim(), { context: picked ? picked.context : '', wants: box.querySelector('#rec-wants').value.trim(), speakers: speakersOn() && speakersReady() }, picked);
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

  root.leoRecording = { pastedScreenshot, saveScreenshot, eventNow, whenOf, MEETING_WANTS, ease, spread, freshLevels, sourceFor, create, clock, stateWord, failureOf, pendingLine, mergePages, earlierFirst, tailMerge, NOT_SPEAKERS, speakersOf, toneOf, pausesOf, transcriptFlow, askChips, liveAsk, briefAsk, FELIX_ASKS, MOST_ASK, unsavedLine, loudness, hearing, hearingWords, preflight, tipWords, HEADSET, QUIET };
})(typeof window !== 'undefined' ? window : globalThis);
