const S = window.leoSchedule;
const cal = { view: 'week', selected: null, data: null, loadedAt: 0, timer: 0, polls: 0, showDone: true, failed: false, drag: null, compose: null, stale: false, peek: null, scrolled: '', suppressClick: false };
const CAL_DONE_KEY = 'leo-calendar-hide-done';
const CAL_VIEW_KEY = 'leo-calendar-view';
const CAL_VIEWS = [['day', 'Day'], ['week', 'Week'], ['month', 'Month']];
try {
  cal.showDone = localStorage.getItem(CAL_DONE_KEY) !== '1';
  const view = localStorage.getItem(CAL_VIEW_KEY);
  if (CAL_VIEWS.some(([v]) => v === view)) cal.view = view;
} catch (e) {
  cal.showDone = true;
}
const CAL_POLL_MS = 150000;
const CAL_TILE_PX = 64;
const CAL_MORE_PX = 30;
const CAL_HOUR_PX = 48;
const CAL_DUE_MIN = Math.ceil(62 / CAL_HOUR_PX * 60);
const CAL_SNAP = 15;
const CAL_HOLD_MS = 300;
const CAL_MONTH_CHIPS = 3;
const KIND_LABEL = { assignment: 'Assignment', exam: 'Exam', meeting: 'Meeting', 'follow-up': 'Follow-up', deadline: 'Deadline', other: 'Other' };
const KIND_ORDER = ['assignment', 'exam', 'meeting', 'follow-up', 'deadline', 'other'];

function calLocale() {
  return navigator.language || 'en-US';
}

function calZone() {
  try {
    return Intl.DateTimeFormat().resolvedOptions().timeZone || '';
  } catch (e) {
    return '';
  }
}

function monthOf(key) {
  const d = S.fromKey(key);
  return new Date(d.getFullYear(), d.getMonth(), 1);
}

function calDays() {
  return S.daysOf(cal.view, cal.selected, S.weekStart(calLocale()));
}

function calWindow() {
  const days = calDays();
  return { days, from: S.fromKey(days[0]).toISOString(), to: S.addDays(S.fromKey(days[days.length - 1]), 1).toISOString() };
}

async function showCalendar() {
  const mine = ++seq;
  state = { view: 'calendar', dir: '' };
  chrome();
  $('#crumbs').innerHTML = '<span class="sep">/</span><button>Calendar</button>';
  document.title = 'Calendar · leo';
  if (!cal.selected) cal.selected = S.dayKey(new Date());
  cal.peek = null;
  cal.scrolled = '';
  cal.ringAt = null;
  app.innerHTML = '<div class="cal-page" id="cal-page"><section class="cal-main" id="cal-main" aria-label="Calendar"></section><aside class="cal-side" id="cal-side"></aside></div>';
  calSized.disconnect();
  calSized.observe($('#cal-main'));
  drawCalendar();
  await loadSchedule(mine);
}

async function loadSchedule(mine = seq) {
  if (state.view !== 'calendar') return;
  const { from, to } = calWindow();
  const asked = `${cal.view} ${from}`;
  cal.loadedFor = asked;
  const taskRevision = tasksWin.revision;
  let data;
  try {
    data = await api(`/api/schedule?from=${enc(from)}&to=${enc(to)}&live=1`);
  } catch (e) {
    if (e instanceof Locked) throw e;
    if (mine === seq && state.view === 'calendar') {
      cal.failed = true;
      drawCalendar();
    }
    return;
  }
  if (mine !== seq || state.view !== 'calendar' || asked !== `${cal.view} ${calWindow().from}`) return;
  if (taskRevision !== tasksWin.revision) data.tasks = tasksWin.tasks || cal.data?.tasks || [];
  clearTimeout(cal.timer);
  if (data.refreshing && cal.polls < 6) {
    cal.polls++;
    cal.timer = setTimeout(() => loadSchedule(mine), 4000);
  } else {
    cal.polls = 0;
    cal.timer = setTimeout(() => {
      if (appVisible()) loadSchedule(mine);
      else cal.missed = true;
    }, CAL_POLL_MS);
  }
  if (cal.drag || cal.compose) {
    cal.stale = true;
    return;
  }
  cal.failed = false;
  cal.data = data;
  tasksWin.tasks = data.tasks || tasksWin.tasks;
  cal.loadedAt = Date.now();
  drawCalendar();
  drawTasksWindow();
}

function calSettled() {
  if (!cal.stale || cal.drag || cal.compose) return;
  cal.stale = false;
  loadSchedule(seq);
}

function calendarShown() {
  if (!appVisible()) return;
  if (state.view === 'calendar' && (cal.missed || Date.now() - cal.loadedAt > 120000)) {
    cal.missed = false;
    cal.loadedAt = Date.now();
    loadSchedule(seq);
  }
  if (tasksWin.stale) loadTasksWindow().catch(() => {});
}
document.addEventListener('visibilitychange', calendarShown);
window.addEventListener('leo-visibility', calendarShown);

function calStatus() {
  const d = cal.data;
  if (cal.failed) return `<span>Couldn't update</span> · <button type="button" class="link-btn" data-action="cal-retry">Retry</button>`;
  if (!d) return '<span>Loading…</span>';
  if (!d.connected) return '';
  if (d.refreshing) return `<span class="activity-spin" aria-hidden="true"></span><span>Updating…</span>`;
  if (!(d.calendars || []).some((c) => c.problem)) return '';
  return '<span>A calendar could not be read</span> · <button type="button" class="link-btn" data-action="cal-retry">Retry</button>';
}

const EVENT_HUES = [0, 1, 2, 3, 6, 7];
const TASK_TONES = { exam: '--hue-red', assignment: '--hue-indigo', meeting: '--hue-violet', 'follow-up': '--hue-teal', deadline: '--hue-amber', other: '--hue-slate' };

function toneOfEvent(event) {
  const key = String(event.title || '').toLowerCase().replace(/\s+/g, ' ').trim();
  let hash = 2166136261;
  for (const ch of key) hash = Math.imul(hash ^ ch.codePointAt(0), 16777619) >>> 0;
  return `--hue-${EVENT_HUES[hash % EVENT_HUES.length]}`;
}

function toneOfTask(task) {
  return TASK_TONES[task.kind] || '--hue-slate';
}

function repeatBubbles(on) {
  return `<div class="repeat-days" role="group" aria-label="Repeat every">${[0, 1, 2, 3, 4, 5, 6].map((d) => {
    const day = new Date(2024, 0, 1 + d);
    const name = day.toLocaleDateString(calLocale(), { weekday: 'long' });
    const letter = day.toLocaleDateString(calLocale(), { weekday: 'narrow' });
    return `<button type="button" class="repeat-day${on.includes(d) ? ' on' : ''}" data-repeat-day="${d}" aria-pressed="${on.includes(d)}" aria-label="${esc(name)}" title="${esc(name)}">${esc(letter)}</button>`;
  }).join('')}</div>`;
}

function repeatHint(repeat) {
  return repeat.length ? S.repeatLabel(repeat, calLocale()) : 'Does not repeat';
}

function repeatPicker(box, start, onChange) {
  const days = [...start];
  box.addEventListener('click', (e) => {
    const b = e.target.closest('[data-repeat-day]');
    if (!b) return;
    const d = Number(b.dataset.repeatDay);
    const at = days.indexOf(d);
    if (at >= 0) days.splice(at, 1);
    else days.push(d);
    days.sort((a, c) => a - c);
    b.classList.toggle('on', at < 0);
    b.setAttribute('aria-pressed', String(at < 0));
    const hint = box.querySelector('.repeat-hint');
    if (hint) hint.textContent = repeatHint(days);
    onChange([...days]);
  });
  return days;
}

function calDate(minutes) {
  const at = new Date(2000, 0, 1);
  at.setMinutes(Math.min(minutes, 24 * 60 - 1));
  return at;
}

function calTime(minutes) {
  return S.timeLabel(S.timeOf(minutes), calLocale());
}

function calHour(minutes) {
  return calDate(minutes).toLocaleTimeString(calLocale(), { hour: 'numeric' });
}

function calRange(start, end) {
  const whole = start % 60 === 0 && end % 60 === 0;
  try {
    return new Intl.DateTimeFormat(calLocale(), whole ? { hour: 'numeric' } : { hour: 'numeric', minute: '2-digit' }).formatRange(calDate(start), calDate(end));
  } catch (e) {
    return `${calTime(start)} – ${calTime(end)}`;
  }
}

function calFind(id) {
  return ((cal.data && cal.data.tasks) || []).find((t) => t.id === id) || null;
}

function calMovable(task) {
  return Boolean(task) && task.found !== false;
}

function calShown(item) {
  return cal.showDone || item.type === 'event' || !item.task.done;
}

function calPlan(days) {
  const today = S.dayKey(new Date());
  return S.plan((cal.data && cal.data.tasks) || [], (cal.data && cal.data.events) || [], today, { from: days[0], to: days[days.length - 1] });
}

function calLabel(item) {
  if (item.type === 'event') return item.event.title || 'Untitled';
  return item.task.title || 'Untitled';
}

function calAttrs(item, day) {
  if (item.type === 'event') return `data-kind="event" data-id="${esc(item.event.id)}"`;
  const t = item.task;
  const on = t.on ? ` data-on="${esc(t.on)}"` : '';
  const drag = calMovable(t) ? ' data-drag="move"' : '';
  return `data-kind="task" data-id="${esc(t.id)}"${on} data-day="${esc(day)}"${drag}`;
}

function calTone(item) {
  if (item.type === 'event') return toneOfEvent(item.event);
  return toneOfTask(item.task);
}

const CAL_RING_MS = 300;
const CAL_RINGS = [
  'M20.3 6.5L21.5 7.7 22.4 9 23.2 10.6 23.6 12.2 23.8 14 23.5 15.7 22.9 17.4 22 18.9 20.8 20.2 19.3 21.3 17.7 22 15.9 22.5 14.1 22.7 12.4 22.7 10.6 22.4 9 21.8 7.4 21.1 6 20.1 4.7 18.9 3.7 17.6 2.8 16.1 2.2 14.4 2 12.7 2.1 10.9 2.6 9.2 3.4 7.5 4.5 6.1 6 4.9 7.6 4 9.3 3.4 11.2 3 13 3 14.8 3.2 16.5 3.7 18.1 4.4 19.7 5.3 21 6.4 22.2 7.8 23.2 9.3 23.9 10.9',
  'M3.6 16.8L2.9 15.2 2.5 13.6 2.4 11.9 2.6 10.2 3.2 8.5 4.1 7 5.4 5.7 6.8 4.6 8.5 3.8 10.3 3.4 12 3.2 13.8 3.3 15.5 3.7 17.2 4.3 18.7 5.1 20.1 6.1 21.3 7.2 22.3 8.6 23.2 10.1 23.7 11.8 23.9 13.5 23.8 15.3 23.3 17 22.5 18.6 21.3 20 19.9 21.2 18.3 22.1 16.5 22.7 14.7 23 12.8 23 11 22.7 9.3 22.2 7.7 21.5 6.2 20.5 4.9 19.4 3.8 18 2.9 16.6 2.2 14.9 1.8 13.2 1.8 11.3',
  'M17 4.1L18.5 5 19.8 6 20.9 7.2 21.8 8.6 22.6 10 23.1 11.6 23.3 13.2 23.3 14.9 22.9 16.6 22.3 18.2 21.3 19.7 20 21 18.5 22 16.8 22.7 14.9 23.1 13.1 23.2 11.2 23 9.5 22.4 7.9 21.6 6.5 20.6 5.3 19.4 4.2 18.1 3.4 16.6 2.8 15 2.5 13.4 2.4 11.7 2.6 9.9 3.2 8.3 4.1 6.7 5.3 5.3 6.8 4.1 8.5 3.3 10.3 2.7 12.2 2.5 14.1 2.7 15.9 3.1 17.6 3.8 19.2 4.8 20.5 6 21.6 7.3',
  'M7.5 4.3L9.2 3.5 10.9 3 12.8 2.8 14.6 2.9 16.4 3.4 18 4.2 19.5 5.2 20.7 6.5 21.6 7.9 22.4 9.3 22.9 10.9 23.1 12.5 23.1 14.1 22.9 15.8 22.4 17.4 21.6 18.9 20.5 20.3 19.2 21.5 17.6 22.5 15.8 23.1 13.9 23.5 12 23.4 10.2 23 8.4 22.3 6.9 21.4 5.6 20.2 4.5 18.8 3.6 17.3 3 15.8 2.7 14.1 2.6 12.4 2.7 10.8 3.1 9.1 3.9 7.5 4.9 6 6.2 4.7 7.7 3.6 9.4 2.8 11.3 2.4 13.3 2.3',
];

function calRing(key, still = false) {
  if (!still) cal.ringAt ??= performance.now();
  const ago = still ? CAL_RING_MS : performance.now() - cal.ringAt;
  const draw = ago < CAL_RING_MS ? ` class="inking" style="animation-delay: ${-Math.round(ago)}ms"` : '';
  return `<svg class="cal-ring" viewBox="0 0 26 26" aria-hidden="true"><path pathLength="1"${draw} d="${CAL_RINGS[S.fromKey(key).getDate() % CAL_RINGS.length]}"/></svg>`;
}

function calCheck(task) {
  const on = task.on ? ` data-on="${esc(task.on)}"` : '';
  return `<input type="checkbox" class="cal-check" data-action="task-done" data-id="${esc(task.id)}"${on} aria-label="Done: ${esc(task.title)}"${task.done ? ` checked${tickWrite(task.id, task.on)}` : ''}>`;
}

function calChip(item, day, { style = '', sub = '', extra = '', tall = false, inline = false } = {}) {
  const task = item.type === 'task' ? item.task : null;
  const kind = task ? 'task' : 'event';
  const done = task && task.done ? ' done' : '';
  const peeked = cal.peek && cal.peek.id === (task ? task.id : item.event.id) && (!task || !task.on || cal.peek.on === task.on) ? ' peeked' : '';
  return `<div class="cal-item ${kind}${done}${peeked}${tall ? ' tall' : ''}${inline ? ' inline' : ''}" ${calAttrs(item, day)} style="--tone: var(${calTone(item)});${style}">
    ${task ? calCheck(task) : ''}
    <button type="button" class="cal-open" data-action="cal-open" ${task ? `data-kind="task" data-id="${esc(task.id)}"${task.on ? ` data-on="${esc(task.on)}"` : ''}` : `data-kind="event" data-id="${esc(item.event.id)}"`} title="${esc(calLabel(item))}${sub ? ` · ${esc(sub)}` : ''}"><span class="cal-title">${esc(calLabel(item))}</span>${sub ? `<span class="cal-sub">${esc(sub)}</span>` : ''}</button>${extra}
  </div>`;
}

function calHead(days) {
  const first = S.fromKey(days[0]);
  const last = S.fromKey(days[days.length - 1]);
  const mid = S.fromKey(cal.view === 'month' ? days[15] : cal.selected);
  let title;
  if (cal.view === 'day') title = `${esc(mid.toLocaleDateString(calLocale(), { weekday: 'long', month: 'long', day: 'numeric' }))} <span>${mid.getFullYear()}</span>`;
  else if (cal.view === 'month' || first.getMonth() === last.getMonth()) title = `${esc(mid.toLocaleDateString(calLocale(), { month: 'long' }))} <span>${mid.getFullYear()}</span>`;
  else title = `${esc(first.toLocaleDateString(calLocale(), { month: 'short' }))} – ${esc(last.toLocaleDateString(calLocale(), { month: 'short' }))} <span>${last.getFullYear()}</span>`;
  const unit = { day: 'day', week: 'week', month: 'month' }[cal.view];
  return `<header class="cal-head">
    <div class="cal-name">${title}</div>
    <div class="cal-step">
      <button type="button" class="icon-btn cal-arrow" data-action="cal-prev" aria-label="Previous ${unit}" title="Previous ${unit} (K)">${ICON.back}</button>
      <button type="button" class="btn sm" data-action="cal-today" title="Today (T)">Today</button>
      <button type="button" class="icon-btn cal-arrow next" data-action="cal-next" aria-label="Next ${unit}" title="Next ${unit} (J)">${ICON.back}</button>
    </div>
    <div class="cal-status" role="status">${calStatus()}</div>
    <div class="cal-views">${segmented({ label: 'View', action: 'cal-view', key: 'view', current: cal.view, choices: CAL_VIEWS.map(([v, label]) => [v, label, '', `${label} (${label[0]})`]) })}</div>
    <button type="button" class="btn sm cal-new" data-action="task-new" data-day="${esc(cal.selected)}">${ICON.plus}<span>New task</span></button>
  </header>`;
}

function calWeekHeads(days, today) {
  return days.map((key) => {
    const d = S.fromKey(key);
    const on = key === today ? ' today' : '';
    return `<button type="button" class="cal-dayhead${on}" data-action="cal-pick" data-day="${key}" data-to="day" aria-label="${esc(d.toLocaleDateString(calLocale(), { weekday: 'long', month: 'long', day: 'numeric' }))}"><span>${esc(d.toLocaleDateString(calLocale(), { weekday: 'short' }))}</span><b>${d.getDate()}${on ? calRing(key) : ''}</b></button>`;
  }).join('');
}

function calAllDay(days, plan) {
  const index = new Map(days.map((k, i) => [k, i]));
  const spans = [];
  const seen = new Set();
  for (const key of days) {
    const day = plan.byDay.get(key);
    if (!day) continue;
    for (const item of day.allDay.filter(calShown)) {
      if (item.type === 'event') {
        if (seen.has(item.event.id)) continue;
        seen.add(item.event.id);
        const covered = S.eventDays(item.event).filter((k) => index.has(k));
        if (!covered.length) continue;
        spans.push({ item, day: covered[0], a: index.get(covered[0]), b: index.get(covered[covered.length - 1]) });
      } else spans.push({ item, day: key, a: index.get(key), b: index.get(key) });
    }
  }
  const lane = S.lanes(spans);
  const rows = Math.max(1, ...lane.map((l) => l + 1));
  const bars = spans.map((s, i) => calChip(s.item, s.day, { style: `grid-column: ${s.a + 1} / ${s.b + 2}; grid-row: ${lane[i] + 1};` })).join('');
  const cells = days.map((key, i) => `<div class="cal-allday-cell" data-day="${key}" style="grid-column: ${i + 1}; grid-row: 1 / ${rows + 1};"></div>`).join('');
  return `<div class="cal-allday" style="--cols: ${days.length}; --rows: ${rows};"><span class="cal-allday-label">All day</span><div class="cal-allday-grid">${cells}${bars}</div></div>`;
}

function calLeast(item) {
  return item.type === 'task' && !item.task.end ? CAL_DUE_MIN : CAL_SNAP * 2;
}

function calColumn(key, plan, today, room = Infinity) {
  const day = plan.byDay.get(key) || { timed: [] };
  const items = day.timed.filter(calShown);
  const spans = items.map((item) => S.spanOn(item, key, calLeast(item)));
  const floor = (item) => 1440 - (item.type === 'task' && !item.task.end ? CAL_DUE_MIN : Math.ceil(20 / CAL_HOUR_PX * 60));
  const placed = spans.map((s, i) => ({ start: Math.min(s.start, floor(items[i])), end: s.end }));
  const cols = S.dayColumns(placed);
  const more = new Map();
  items.forEach((item, i) => {
    if (cols[i].col < room) return;
    const under = items.findIndex((x, j) => cols[j].col === room - 1 && placed[j].start < placed[i].end && placed[i].start < placed[j].end);
    if (under >= 0) more.set(under, (more.get(under) || 0) + 1);
  });
  const area = more.size ? `(100% - ${CAL_MORE_PX}px)` : '100%';
  const blocks = items.map((item, i) => {
    if (cols[i].col >= room) return '';
    const { start, end } = spans[i];
    const { col } = cols[i];
    const n = Math.min(cols[i].cols, room);
    const step = Math.min(40, 70 / n);
    const left = col * step;
    const width = col === n - 1 ? 100 - left : Math.min(100 - left, step * 1.8);
    const top = (placed[i].start / 60) * CAL_HOUR_PX;
    const height = Math.max(18, ((end - placed[i].start) / 60) * CAL_HOUR_PX - 2);
    const length = end - placed[i].start;
    const task = item.type === 'task' ? item.task : null;
    const sub = length <= 30 && !task ? '' : task && !task.end ? `Due ${calTime(start)}` : calRange(start, item.type === 'event' ? end : S.minutesOf(task.end));
    const grip = task && task.source === 'leo' && calMovable(task) ? '<span class="cal-grip" data-drag="resize" aria-hidden="true"></span>' : '';
    const rest = more.get(i) ? `<button type="button" class="cal-stack-more" data-action="cal-pick" data-day="${key}" data-to="day" style="top: ${top + 2}px; z-index: ${room + 2};" aria-label="${more.get(i)} more at this time, open the day">+${more.get(i)}</button>` : '';
    return calChip(item, key, { style: ` top: ${top}px; height: ${height}px; left: calc(${area} * ${left / 100} + 2px); width: calc(${area} * ${width / 100} - 5px); z-index: ${col + 1};`, sub, extra: grip, tall: end - start >= 90, inline: Boolean(task) && length < 60 }) + rest;
  }).join('');
  let now = '';
  if (key === today) {
    const at = new Date();
    const top = ((at.getHours() * 60 + at.getMinutes()) / 60) * CAL_HOUR_PX;
    now = `<div class="cal-past" style="height: ${top}px" aria-hidden="true"></div><div class="cal-now" style="top: ${top}px" aria-label="Now"><svg class="cal-now-drop" aria-hidden="true"><use href="#leo-mark"/></svg></div>`;
  }
  const wknd = [0, 6].includes(S.fromKey(key).getDay()) ? ' weekend' : '';
  return `<div class="cal-col${wknd}" data-day="${key}">${blocks}${now}</div>`;
}

function calRoom(count) {
  if (count < 2) return Infinity;
  const width = document.querySelector('#cal-scroll .cal-cols')?.clientWidth || Math.max(0, ($('#cal-main')?.clientWidth || innerWidth - 700) - 52);
  return Math.max(1, Math.floor((width / count - CAL_MORE_PX) / CAL_TILE_PX));
}

function calFit() {
  if (state.view !== 'calendar' || cal.view === 'month' || cal.drag || cal.compose || !document.querySelector('#cal-scroll .cal-cols')) return;
  if (calRoom(calDays().length) !== cal.drawnRoom) drawCalendar();
}

const calSized = new ResizeObserver(calFit);

function calTimeGrid(days, plan, today) {
  const room = calRoom(days.length);
  cal.drawnRoom = room;
  const hours = Array.from({ length: 23 }, (_, i) => `<span style="top: ${(i + 1) * CAL_HOUR_PX}px">${esc(calHour((i + 1) * 60))}</span>`).join('');
  return `<div class="cal-week" style="--cols: ${days.length}; --hour: ${CAL_HOUR_PX}px;">
    <div class="cal-dayheads"><span></span>${calWeekHeads(days, today)}</div>
    ${calAllDay(days, plan)}
    <div class="cal-scroll" id="cal-scroll"><div class="cal-hours" aria-hidden="true">${hours}</div><div class="cal-cols">${days.map((k) => calColumn(k, plan, today, room)).join('')}</div></div>
  </div>`;
}

function calMonth(days, plan, today) {
  const month = S.fromKey(days[15]).getMonth();
  const names = days.slice(0, 7).map((k) => `<span>${esc(S.fromKey(k).toLocaleDateString(calLocale(), { weekday: 'short' }))}</span>`).join('');
  const cells = days.map((key) => {
    const d = S.fromKey(key);
    const day = plan.byDay.get(key) || { allDay: [], timed: [] };
    const items = [...day.allDay, ...day.timed].filter(calShown);
    const shown = items.slice(0, CAL_MONTH_CHIPS).map((item) => {
      const time = item.type === 'event' ? (item.event.all_day ? '' : calTime(S.spanOn(item, key).start)) : item.task.time ? calTime(S.minutesOf(item.task.time)) : '';
      return calChip(item, key, { sub: time });
    }).join('');
    const more = items.length > CAL_MONTH_CHIPS ? `<button type="button" class="link-btn cal-more" data-action="cal-pick" data-day="${key}" data-to="day">${items.length - CAL_MONTH_CHIPS} more</button>` : '';
    const cls = `${d.getMonth() === month ? '' : ' out'}${key === today ? ' today' : ''}`;
    return `<div class="cal-cell${cls}" data-day="${key}"><button type="button" class="cal-num" data-action="cal-pick" data-day="${key}" data-to="day" aria-label="${esc(d.toLocaleDateString(calLocale(), { weekday: 'long', month: 'long', day: 'numeric' }))}">${d.getDate()}${key === today ? calRing(key) : ''}</button>${shown}${more}</div>`;
  }).join('');
  return `<div class="cal-month"><div class="cal-month-names">${names}</div><div class="cal-month-grid">${cells}</div></div>`;
}

function calMini(today) {
  const at = S.fromKey(cal.selected);
  const grid = S.monthGrid(at.getFullYear(), at.getMonth(), S.weekStart(calLocale()));
  const shown = new Set(calDays());
  const names = grid.slice(0, 7).map((c) => `<span>${esc(S.fromKey(c.key).toLocaleDateString(calLocale(), { weekday: 'narrow' }))}</span>`).join('');
  return `<section class="cal-mini" aria-label="${esc(at.toLocaleDateString(calLocale(), { month: 'long', year: 'numeric' }))}">
    <div class="cal-mini-head"><span>${esc(at.toLocaleDateString(calLocale(), { month: 'long', year: 'numeric' }))}</span><button type="button" class="icon-btn cal-arrow" data-action="cal-mini" data-step="-1" aria-label="Previous month">${ICON.back}</button><button type="button" class="icon-btn cal-arrow next" data-action="cal-mini" data-step="1" aria-label="Next month">${ICON.back}</button></div>
    <div class="cal-mini-grid">${names}${grid.map((c) => `<button type="button" class="cal-mini-day${c.inMonth ? '' : ' out'}${c.key === today ? ' today' : ''}${cal.view !== 'month' && shown.has(c.key) ? ' shown' : ''}" data-action="cal-pick" data-day="${c.key}">${c.day}${c.key === today ? calRing(c.key) : ''}</button>`).join('')}</div>
  </section>`;
}

function calTrayRow(task) {
  const when = task.date ? S.dayWord(task.date, new Date(), calLocale()) : '';
  return `<div class="cal-tray-row" data-kind="task" data-id="${esc(task.id)}"${calMovable(task) ? ' data-drag="tray"' : ''} style="--tone: var(${task.date ? '--hue-red' : toneOfTask(task)})">${calCheck(task)}<button type="button" class="cal-open" data-action="cal-open" data-kind="task" data-id="${esc(task.id)}"><span class="cal-title">${esc(task.title)}</span>${when ? `<span class="cal-sub" aria-hidden="true" title="${esc(S.dayName(task.date, calLocale()))}">${esc(when)}</span><span class="said">, ${esc(S.dayName(task.date, calLocale()))}</span>` : ''}</button></div>`;
}

function calTray(plan, today) {
  const d = cal.data;
  const connect = d && !d.connected ? '<p class="set-note cal-connect">Show your Google Calendar here · <a href="#/settings/calendar">Connect it in Settings</a></p>' : '';
  const list = (title, tasks, late) => tasks.length ? `<section class="cal-tray-list"><h2${late ? ' class="late"' : ''}>${title} <span class="cal-count">${tasks.length}</span></h2>${tasks.slice(0, 50).map(calTrayRow).join('')}</section>` : '';
  const undated = list('No date', plan.undated, false);
  const tasks = (d && d.tasks) || [];
  const toggle = tasks.some((t) => t.done || (t.done_on || []).length) ? `<button type="button" class="link-btn cal-done-toggle" data-action="cal-done">${cal.showDone ? 'Hide completed' : 'Show completed'}</button>` : '';
  return `${calMini(today)}<div class="cal-tasks-head"><h2>Tasks</h2><button type="button" class="icon-btn" data-action="task-new" aria-label="New task" title="New task">${ICON.plus}</button></div>${list('Earlier', plan.overdue, true)}${undated}${!plan.overdue.length && !plan.undated.length ? `${DRAWN.tasks}<p class="hint cal-empty">Nothing from earlier. Tasks without a date land here, ready to drag onto the calendar.</p>` : ''}${plan.undated.length ? '<p class="hint cal-hint">Drag a task onto the calendar to give it a time</p>' : ''}${toggle}${connect}`;
}

function drawCalendar() {
  const main = $('#cal-main');
  if (!main) return;
  const today = S.dayKey(new Date());
  const days = calDays();
  const plan = calPlan(days.includes(today) ? days : [...days, today].sort());
  const body = cal.view === 'month' ? calMonth(days, plan, today) : calTimeGrid(days, plan, today);
  const scroll = $('#cal-scroll') || $('.cal-month-grid');
  const keep = scroll ? scroll.scrollTop : null;
  const views = main.querySelector('.cal-views');
  main.innerHTML = `${calHead(days)}${body}`;
  if (views?.querySelector('.segment.on')?.dataset.view === cal.view) main.querySelector('.cal-views').replaceWith(views);
  main.dataset.view = cal.view;
  settleNow('cal', (id) => app.querySelector(`.cal-item[data-id="${CSS.escape(id)}"], .cal-tray-row[data-id="${CSS.escape(id)}"]`));
  const box = $('#cal-scroll') || $('.cal-month-grid');
  if (box) {
    const place = `${cal.view} ${days[0]}`;
    if (keep !== null && cal.scrolled === place) box.scrollTop = keep;
    else if (cal.view === 'month') cal.scrolled = place;
    else {
      const now = new Date();
      const hour = days.includes(today) ? Math.max(0, now.getHours() - 1) : 8;
      box.scrollTop = hour * CAL_HOUR_PX;
      cal.scrolled = place;
    }
  }
  if (!cal.peek) {
    const side = $('#cal-side');
    if (side) {
      side.classList.remove('peek');
      side.innerHTML = calTray(plan, today);
    }
  }
  inkSettle((id) => app.querySelectorAll(`.cal-item[data-id="${CSS.escape(id)}"], .cal-tray-row[data-id="${CSS.escape(id)}"]`));
  requestAnimationFrame(calFit);
}

function calStep(step) {
  const at = S.fromKey(cal.selected);
  if (cal.view === 'day') cal.selected = S.dayKey(S.addDays(at, step));
  else if (cal.view === 'week') cal.selected = S.dayKey(S.addDays(at, step * 7));
  else {
    const m = new Date(at.getFullYear(), at.getMonth() + step, 1);
    const keep = Math.min(at.getDate(), new Date(m.getFullYear(), m.getMonth() + 1, 0).getDate());
    cal.selected = S.dayKey(new Date(m.getFullYear(), m.getMonth(), keep));
  }
  calGo();
}

function calGo() {
  drawCalendar();
  if (cal.loadedFor !== `${cal.view} ${calWindow().from}`) loadSchedule(seq);
}

function calSetView(view) {
  if (!CAL_VIEWS.some(([v]) => v === view) || view === cal.view) return;
  const from = CAL_VIEWS.findIndex(([v]) => v === cal.view);
  overlapOut($('#cal-main > :last-child'));
  cal.view = view;
  try {
    localStorage.setItem(CAL_VIEW_KEY, view);
  } catch (e) {
    cal.view = view;
  }
  calGo();
  const views = $('.cal-views .segmented');
  if (views) syncSegmented(views, from);
  cascade($('#cal-main'), 'swap-in');
}

document.addEventListener('keydown', (e) => {
  if (state.view !== 'calendar' || e.metaKey || e.ctrlKey || e.altKey || e.isComposing) return;
  if (e.key === 'Escape') {
    if (cal.drag) {
      e.preventDefault();
      e.stopImmediatePropagation();
      calDragCancel();
      return;
    }
    if (cal.peek && !$('.scrim') && !e.target.closest?.('input, textarea, select')) {
      e.preventDefault();
      e.stopImmediatePropagation();
      calClosePeek();
    }
    return;
  }
  if (e.target.closest?.('input, textarea, select, [contenteditable], .scrim') || $('.scrim')) return;
  const key = e.key.toLowerCase();
  const act = { t: () => { cal.selected = S.dayKey(new Date()); calGo(); }, j: () => calStep(1), k: () => calStep(-1), d: () => calSetView('day'), w: () => calSetView('week'), m: () => calSetView('month') }[key];
  if (!act) return;
  e.preventDefault();
  act();
});

const calHM = (t) => (t ? String(t).slice(0, 5) : null);

function calMinutesIn(col, y) {
  const rect = col.getBoundingClientRect();
  return ((y - rect.top) / CAL_HOUR_PX) * 60;
}

function calColumnAt(x, y) {
  const cols = $('.cal-cols');
  if (!cols) return null;
  const box = cols.getBoundingClientRect();
  const scroll = $('#cal-scroll');
  const view = scroll ? scroll.getBoundingClientRect() : box;
  if (y < view.top || y > view.bottom) return null;
  return [...cols.children].find((c) => {
    const r = c.getBoundingClientRect();
    return x >= r.left && x < r.right;
  }) || null;
}

function calTargetAt(d) {
  const { x, y } = d;
  const col = calColumnAt(x, y);
  if (col) {
    const raw = calMinutesIn(col, y);
    return { day: col.dataset.day, col, minutes: Math.max(0, Math.min(24 * 60 - d.length, S.snap(raw - d.offset, CAL_SNAP))) };
  }
  const grid = document.elementFromPoint(x, y)?.closest('.cal-allday-grid');
  if (grid) {
    const cells = [...grid.querySelectorAll('.cal-allday-cell')];
    const cell = cells.find((c) => {
      const r = c.getBoundingClientRect();
      return x >= r.left && x < r.right;
    });
    if (cell) return { day: cell.dataset.day, cell, minutes: null };
  }
  const cell = document.elementFromPoint(x, y)?.closest('.cal-cell');
  if (cell) return { day: cell.dataset.day, cell, minutes: 'keep' };
  return null;
}

function calClearMarks() {
  for (const el of document.querySelectorAll('.cal-slot')) el.remove();
  for (const el of document.querySelectorAll('.drop-on')) el.classList.remove('drop-on');
}

function calSlot(col, start, end) {
  const slot = document.createElement('div');
  slot.className = 'cal-slot';
  slot.style.top = `${(start / 60) * CAL_HOUR_PX}px`;
  slot.style.height = `${Math.max(16, ((end - start) / 60) * CAL_HOUR_PX - 2)}px`;
  slot.textContent = calRange(start, end);
  col.appendChild(slot);
}

function calDragTask(d) {
  const base = calFind(d.id);
  if (!base) return null;
  if (!d.on || !d.day) return base;
  return S.occurrences(base, d.day, d.day).find((o) => o.on === d.on) || null;
}

function calLift() {
  const d = cal.drag;
  if (!d || d.live) return;
  d.live = true;
  clearTimeout(d.timer);
  getSelection()?.removeAllRanges();
  document.addEventListener('touchmove', calHoldStill, { passive: false });
  document.body.classList.add('cal-dragging');
  if (d.mode === 'move' || d.mode === 'tray') {
    d.source?.classList.add('lifted');
    const ghost = document.createElement('div');
    ghost.className = 'cal-ghost';
    ghost.setAttribute('aria-hidden', 'true');
    ghost.style.setProperty('--tone', getComputedStyle(d.source).getPropertyValue('--tone') || 'var(--hue-slate)');
    ghost.textContent = d.task.title || 'Untitled';
    document.body.appendChild(ghost);
    d.ghost = ghost;
    lift(ghost);
  }
  try { app.setPointerCapture(d.pointer); } catch (e) {}
  calFollow();
}

function calHoldStill(e) {
  if (cal.drag?.live && e.cancelable) e.preventDefault();
}

function calEdge(d) {
  const scroll = $('#cal-scroll');
  if (!scroll) return;
  const r = scroll.getBoundingClientRect();
  if (d.y < r.top + 36 && d.y > r.top - 40) scroll.scrollTop -= 12;
  else if (d.y > r.bottom - 36 && d.y < r.bottom + 40) scroll.scrollTop += 12;
}

function calFollow() {
  const d = cal.drag;
  if (!d || !d.live) return;
  calEdge(d);
  calClearMarks();
  if (d.ghost) {
    const page = app.getBoundingClientRect();
    d.ghost.style.transform = `translate(${pastEdge(d.x, page.left, page.right) + 10}px, ${pastEdge(d.y, page.top, page.bottom) + 6}px)`;
  }
  if (d.mode === 'resize') {
    const before = d.task.end ? S.minutesOf(d.task.end) : d.start + d.length;
    const moved = d.y === d.startY ? 0 : calMinutesIn(d.col, d.y) - d.start - d.offset;
    const end = moved ? Math.max(d.start + CAL_SNAP, Math.min(24 * 60, S.snap(before + moved, CAL_SNAP))) : before;
    d.target = moved ? { day: d.day, minutes: d.start, end } : null;
    d.source.style.height = `${Math.max(18, ((end - d.start) / 60) * CAL_HOUR_PX - 2)}px`;
    d.source.classList.toggle('inline', end - d.start < 60);
    const sub = d.source.querySelector('.cal-sub');
    if (sub) sub.textContent = calRange(d.start, end);
    return;
  }
  if (d.mode === 'create') {
    const now = Math.max(0, Math.min(24 * 60, S.snap(calMinutesIn(d.col, d.y), CAL_SNAP)));
    const start = Math.min(d.start, now);
    const end = Math.max(d.start, now, start + CAL_SNAP);
    d.target = { day: d.day, minutes: start, end };
    calSlot(d.col, start, end);
    return;
  }
  const at = calTargetAt(d);
  d.target = at;
  if (!at) return;
  if (at.col) calSlot(at.col, at.minutes, at.minutes + d.length);
  else (at.cell || at.col)?.classList.add('drop-on');
}

function calDragEnd() {
  const d = cal.drag;
  if (!d) return;
  clearTimeout(d.timer);
  document.removeEventListener('touchmove', calHoldStill);
  d.settled = liftedRect(d.ghost);
  d.ghost?.remove();
  d.source?.classList.remove('lifted');
  calClearMarks();
  document.body.classList.remove('cal-dragging');
  cal.drag = null;
}

function calDragCancel() {
  const d = cal.drag;
  calDragEnd();
  if (d?.live) drawCalendar();
  calSettled();
}

function calDrop() {
  const d = cal.drag;
  if (!d) return;
  const target = d.target;
  calDragEnd();
  cal.suppressClick = true;
  setTimeout(() => { cal.suppressClick = false; }, 0);
  if (!target) {
    drawCalendar();
    return calSettled();
  }
  if (d.mode === 'create') {
    drawCalendar();
    return calCompose(target.day, target.minutes, target.end);
  }
  const task = d.task;
  if (d.mode !== 'resize') settleLater('cal', task.id, d.settled);
  let next;
  if (d.mode === 'resize') next = { date: target.day, time: calHM(task.time), end: S.timeOf(target.end) };
  else if (target.minutes === 'keep') next = { date: target.day, time: calHM(task.time), end: calHM(task.end) };
  else if (target.minutes === null) next = { date: target.day, time: null, end: null };
  else {
    const keepsEnd = task.source === 'leo' && (task.end || !task.time);
    next = { date: target.day, time: S.timeOf(target.minutes), end: keepsEnd ? S.timeOf(Math.min(24 * 60 - 1, target.minutes + d.length)) : null };
  }
  if (next.date === task.date && next.time === calHM(task.time) && next.end === calHM(task.end)) {
    drawCalendar();
    return calSettled();
  }
  return calMove(task, next);
}

async function calMove(task, next, scope = null) {
  const base = calFind(task.id);
  if (!base) return calSettled();
  if (S.repeats(base) && !scope) {
    drawCalendar();
    return calAskScope(task, next);
  }
  return calSave(base, task, next, scope || 'one');
}

function calAskScope(task, next) {
  const box = sheet(`<h3>Move “${esc(task.title)}”</h3><p class="hint">It repeats ${esc(S.repeatLabel(task.repeat, calLocale()).toLowerCase())}.</p>
    <div class="buttons"><button type="button" class="btn plain" data-action="close">Cancel</button><button type="button" class="btn" id="cal-move-all">Every time</button><button type="button" class="btn primary" id="cal-move-one">Only this one</button></div>`);
  $('#cal-move-one', box).addEventListener('click', () => { closeSheet(); calMove(task, next, 'one').catch(fail); });
  $('#cal-move-all', box).addEventListener('click', () => { closeSheet(); calMove(task, next, 'all').catch(fail); });
  $('#cal-move-one', box).focus();
  calSettled();
}

function calBodies(base, task, next, scope) {
  if (S.repeats(base) && scope === 'one') {
    const on = task.on;
    const was = (base.moved || {})[on] || { date: on, time: base.time, end: base.end };
    return {
      body: { occurrence: on, date: next.date, time: next.time, end: next.time ? next.end : null },
      undo: { occurrence: on, date: was.date, time: calHM(was.time), end: calHM(was.end) },
      local: (t) => { t.moved = { ...(t.moved || {}), [on]: { date: next.date, time: next.time, end: next.end } }; },
    };
  }
  if (S.repeats(base)) {
    const shift = Math.round((S.fromKey(next.date) - S.fromKey(task.date)) / 86400000);
    const start = S.dayKey(S.addDays(S.fromKey(base.date || task.date), shift));
    const repeat = S.shiftRepeat(base.repeat, shift);
    return {
      body: { date: start, time: next.time, end: next.end, repeat },
      undo: { date: base.date, time: calHM(base.time), end: calHM(base.end), repeat: base.repeat },
      local: (t) => Object.assign(t, { date: start, time: next.time, end: next.end, repeat }),
    };
  }
  return {
    body: { ...next },
    undo: { date: base.date, time: calHM(base.time), end: calHM(base.end), confirm: true },
    local: (t) => Object.assign(t, next),
  };
}

async function calSave(base, task, next, scope) {
  base = calFind(base.id) || base;
  const { body, undo, local } = calBodies(base, task, next, scope);
  const before = JSON.parse(JSON.stringify(base));
  local(base);
  tasksWin.revision++;
  drawCalendar();
  try {
    const saved = await api(`/api/tasks/${enc(base.id)}`, { method: 'PATCH', body });
    if (saved && saved.id) {
      for (const key of Object.keys(base)) if (!(key in saved)) delete base[key];
      Object.assign(base, saved);
    }
  } catch (e) {
    for (const key of Object.keys(base)) delete base[key];
    Object.assign(base, before);
    drawCalendar();
    calSettled();
    if (e instanceof Locked) throw e;
    if (e.status === 409 && e.data && e.data.preview) return saveDate(base, next, false);
    toast(e.message, { bad: true });
    return;
  }
  calPeekRefresh(base.id);
  toast(next.date ? `Moved to ${S.fromKey(next.date).toLocaleDateString(calLocale(), { weekday: 'short', month: 'short', day: 'numeric' })}${next.time ? `, ${S.timeLabel(next.time, calLocale())}` : ''}` : 'Date cleared', {
    action: 'Undo',
    run: () => api(`/api/tasks/${enc(base.id)}`, { method: 'PATCH', body: undo }).then(() => { loadTasksWindow().catch(() => {}); return loadSchedule(seq); }).then(() => calPeekRefresh(base.id)).catch(fail),
  });
  loadTasksWindow().catch(() => {});
  cal.stale = true;
  calSettled();
}

function calCompose(day, start, end) {
  calCloseCompose();
  const timed = start !== null && start !== undefined;
  const host = timed ? app.querySelector(`.cal-col[data-day="${CSS.escape(day)}"]`) : app.querySelector(`.cal-cell[data-day="${CSS.escape(day)}"], .cal-allday-cell[data-day="${CSS.escape(day)}"]`);
  if (!host) return calSettled();
  const box = document.createElement('form');
  box.className = `cal-compose${timed ? '' : ' all-day'}`;
  if (timed) {
    box.style.top = `${(start / 60) * CAL_HOUR_PX}px`;
    box.style.height = `${Math.max(34, ((end - start) / 60) * CAL_HOUR_PX - 2)}px`;
  }
  box.innerHTML = `<input class="cal-compose-input" maxlength="300" placeholder="New task" aria-label="New task${timed ? `, ${esc(calRange(start, end))}` : ''}" autocomplete="off">${timed ? `<span class="cal-sub">${esc(calRange(start, end))}</span>` : ''}`;
  host.appendChild(box);
  const input = box.querySelector('input');
  cal.compose = { day, start: timed ? start : null, end: timed ? end : null, box, sent: false };
  const finish = async () => {
    const c = cal.compose;
    if (!c || c.box !== box || c.sent) return;
    const title = input.value.trim();
    if (!title) return calCloseCompose();
    c.sent = true;
    const body = { title, date: day, time: timed ? S.timeOf(start) : null, end: timed ? S.timeOf(end) : null, kind: 'other' };
    try {
      const made = await api('/api/tasks', { method: 'POST', body });
      inkLand(made.id);
      toast(`Added “${made.title}”`, { action: 'Undo', run: () => api(`/api/tasks/${enc(made.id)}`, { method: 'DELETE' }).then(() => loadSchedule(seq)).catch(fail) });
      loadTasksWindow().catch(() => {});
      cal.stale = true;
    } catch (e) {
      if (e instanceof Locked) throw e;
      toast(e.message, { bad: true });
    }
    calCloseCompose();
  };
  box.addEventListener('submit', (e) => { e.preventDefault(); finish().catch(fail); });
  input.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      calCloseCompose();
    }
  });
  input.addEventListener('blur', () => setTimeout(() => finish().catch(fail), 0));
  input.focus({ preventScroll: true });
}

function calCloseCompose() {
  const c = cal.compose;
  if (!c) return;
  cal.compose = null;
  c.box.remove();
  calSettled();
}

document.addEventListener('pointerdown', (e) => {
  if (state.view !== 'calendar' || e.button !== 0 || cal.drag || !e.isPrimary) return;
  if (e.target.closest('.cal-compose')) return;
  const grip = e.target.closest('.cal-grip');
  const item = grip ? grip.closest('.cal-item') : e.target.closest('.cal-item[data-drag], .cal-tray-row[data-drag]');
  const touch = e.pointerType === 'touch';
  let d = null;
  if (item) {
    if (!grip && e.target.closest('input, a, select, textarea')) return;
    const task = calDragTask({ id: item.dataset.id, on: item.dataset.on || null, day: item.dataset.day || null });
    if (!task) return;
    const col = item.closest('.cal-col');
    const span = col ? S.spanOn({ type: 'task', task }, col.dataset.day, calLeast({ type: 'task', task })) : null;
    const length = span ? span.end - span.start : task.end && task.time ? S.minutesOf(task.end) - S.minutesOf(task.time) : 60;
    d = { mode: grip ? 'resize' : item.classList.contains('cal-tray-row') ? 'tray' : 'move', id: task.id, on: task.on || null, task, source: item, col, day: item.dataset.day || null, start: span ? span.start : 0, length: Math.max(CAL_SNAP, length), offset: span ? calMinutesIn(col, e.clientY) - span.start : 0 };
  } else if (e.target.classList.contains('cal-col')) {
    const col = e.target;
    const raw = calMinutesIn(col, e.clientY);
    const slack = raw % 60 >= 58 ? 2 : 0;
    d = { mode: 'create', col, day: col.dataset.day, start: Math.min(24 * 60 - CAL_SNAP, Math.floor((raw + slack) / CAL_SNAP) * CAL_SNAP) };
  } else if (e.target.classList.contains('cal-cell') || e.target.classList.contains('cal-allday-cell')) {
    d = { mode: 'cell', day: e.target.dataset.day };
  }
  if (!d) return;
  if (cal.compose) calCloseCompose();
  cal.drag = { ...d, pointer: e.pointerId, startX: e.clientX, startY: e.clientY, x: e.clientX, y: e.clientY, live: false, timer: 0, touch };
  if (touch && d.mode !== 'cell') cal.drag.timer = setTimeout(calLift, CAL_HOLD_MS);
  else if (!touch) e.preventDefault();
});

document.addEventListener('pointermove', (e) => {
  const d = cal.drag;
  if (!d || e.pointerId !== d.pointer) return;
  d.x = e.clientX;
  d.y = e.clientY;
  if (!d.live) {
    const far = Math.hypot(d.x - d.startX, d.y - d.startY);
    if (d.touch) { if (far > 8) calDragEnd(); return; }
    if (far < 5 || d.mode === 'cell') return;
    if (!e.buttons) { calDragEnd(); return; }
    calLift();
    return;
  }
  if (!e.buttons && e.pointerType !== 'touch') { calDrop(); return; }
  calFollow();
});

document.addEventListener('pointerup', (e) => {
  const d = cal.drag;
  if (!d || e.pointerId !== d.pointer) return;
  if (d.live) return Promise.resolve(calDrop()).catch(fail);
  calDragEnd();
  if (d.mode === 'create') calCompose(d.day, d.start, Math.min(24 * 60, d.start + 60));
  else if (d.mode === 'cell') calCompose(d.day, null, null);
  else calSettled();
});

document.addEventListener('pointercancel', (e) => {
  const d = cal.drag;
  if (!d || e.pointerId !== d.pointer) return;
  if (d.live) calDragCancel();
  else calDragEnd();
});

app.addEventListener('lostpointercapture', (e) => {
  if (cal.drag?.live && e.pointerId === cal.drag.pointer) Promise.resolve(calDrop()).catch(fail);
});

document.addEventListener('click', (e) => {
  if (!cal.suppressClick) return;
  e.preventDefault();
  e.stopImmediatePropagation();
}, true);

function calOpenPeek(kind, id, on = null) {
  const side = $('#cal-side');
  if (!side) return;
  cal.peek = { kind, id, on };
  side.classList.add('peek');
  if (kind === 'event') calPeekEvent(side, id);
  else calPeekTask(side, id, on);
  drawCalendar();
}

function calClosePeek() {
  if (!cal.peek) return;
  const flush = cal.peek.flush;
  cal.peek = null;
  if (flush) flush();
  drawCalendar();
}

function calPeekRefresh(id) {
  if (!cal.peek || cal.peek.id !== id || cal.peek.kind !== 'task') return;
  const side = $('#cal-side');
  if (!side) return;
  if (!side.contains(document.activeElement)) return calPeekTask(side, id, cal.peek.on);
  const task = calFind(id);
  const google = $('#cal-peek-google', side);
  const acts = $('#cal-peek-acts', side);
  if (!task || !google || !acts) return;
  google.textContent = calPeekGoogle(task);
  acts.innerHTML = calPeekActs(task);
}

function calOnGoogle(task) {
  return (cal.data && (cal.data.events || []).find((e) => e.task === task.id)) || null;
}

function calPeekGoogle(task) {
  return calOnGoogle(task) ? 'On Google Calendar' : task.sent_at ? 'Opened in Google Calendar' : 'Not added';
}

function calPeekActs(task) {
  const leo = task.source === 'leo';
  return `${task.date && !calOnGoogle(task) ? `<button type="button" class="icon-btn" aria-label="Add to Google Calendar" title="Add to Google Calendar" data-action="task-google" data-id="${esc(task.id)}">${ICON.calendar}</button>` : ''}
      ${task.date ? `<a class="icon-btn" href="/api/tasks/${enc(task.id)}/ics" download aria-label="Download .ics" title="Download .ics">${ICON.download}</a>` : ''}
      ${leo ? `<button type="button" class="icon-btn danger" aria-label="Delete" title="Delete" data-action="task-delete" data-id="${esc(task.id)}">${ICON.trash}</button>` : task.promoted ? `<button type="button" class="icon-btn" aria-label="Stop tracking" title="Stop tracking" data-action="task-delete" data-id="${esc(task.id)}">${ICON.close}</button>` : ''}`;
}

function calPeekBar(where, actions = '') {
  return `<div class="cal-peek-bar"><span class="cal-peek-where"><span class="cal-dot" aria-hidden="true"></span><span>${esc(where)}</span></span><span class="hint grow" id="cal-peek-saved" role="status"></span>${actions}<button type="button" class="icon-btn" data-action="cal-peek-close" aria-label="Close" title="Close (Esc)">${ICON.close}</button></div>`;
}

function eventDetailsHtml(event) {
  const when = event.all_day
    ? S.fromKey(event.date).toLocaleDateString(calLocale(), { weekday: 'long', month: 'long', day: 'numeric' })
    : `${new Date(event.start).toLocaleString(calLocale(), { weekday: 'short', month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit' })} – ${new Date(event.end).toLocaleTimeString(calLocale(), { hour: 'numeric', minute: '2-digit' })}`;
  const about = S.eventDetails(event.context);
  const where = event.location || about.place;
  const linked = (text) => (/^https?:\/\/\S+$/i.test(text) ? `<a href="${esc(text)}" target="_blank" rel="noopener noreferrer">${esc(text)}</a>` : esc(text));
  return `<div class="set-row"><span class="set-label">When</span><span class="set-value">${esc(when)}</span></div>
    ${where ? `<div class="set-row"><span class="set-label">Where</span><span class="set-value sched-wrap">${linked(where)}</span></div>` : ''}
    <div class="set-row"><span class="set-label">Calendar</span><span class="set-value">${esc(event.calendar || 'Google')}</span></div>
    ${about.organiser ? `<div class="set-row"><span class="set-label">Organiser</span><span class="set-value sched-wrap">${esc(about.organiser)}</span></div>` : ''}
    ${about.invited.length ? `<div class="set-row"><span class="set-label">Invited</span><span class="set-value sched-people">${about.invited.map((p) => `<span class="chip">${esc(p)}</span>`).join('')}</span></div>` : ''}
    ${about.description ? `<div class="sched-about prose">${S.richText(about.description)}</div>` : ''}
    ${event.zone_unknown ? '<p class="set-note warn">This event names a time zone leo does not know, so its time may be off.</p>' : ''}`;
}

function calEventSoon(event) {
  return !event.all_day && new Date(event.start) - Date.now() < 15 * 60000 && new Date(event.end) > Date.now();
}

function calPeekEvent(side, id) {
  const event = ((cal.data && cal.data.events) || []).find((e) => e.id === id);
  if (!event) return calClosePeek();
  side.style.setProperty('--tone', `var(${toneOfEvent(event)})`);
  side.innerHTML = `${calPeekBar(event.calendar || 'Google Calendar')}<h2 class="cal-peek-title">${esc(event.title)}</h2>${eventDetailsHtml(event)}
    <p class="hint">Events from your calendar link are read-only here.</p>
    ${calEventSoon(event) ? '<div class="buttons"><button type="button" class="btn primary" data-action="record">Record this meeting</button></div>' : ''}`;
}

function calPeekTask(side, id, on) {
  const task = calFind(id);
  if (!task) return calClosePeek();
  const repeating = S.repeats(task);
  const day = repeating ? on || S.dayKey(new Date()) : null;
  const slot = repeating ? (task.moved || {})[day] || { date: day, time: task.time, end: task.end } : task;
  const leo = task.source === 'leo';
  const doneNow = taskDoneOn(task, day);
  const dayName = day ? S.fromKey(day).toLocaleDateString(calLocale(), { weekday: 'short', month: 'short', day: 'numeric' }) : '';
  const scope = (cal.peek && cal.peek.scope) || 'one';
  side.style.setProperty('--tone', `var(${toneOfTask(task)})`);
  side.innerHTML = `${calPeekBar(task.note ? task.note.title || 'Untitled' : 'Task')}
    <textarea class="kanban-sheet-title cal-peek-title" id="cal-peek-title" rows="1" maxlength="300" aria-label="Title"${leo ? '' : ' readonly'}>${esc(task.title)}</textarea>
    ${task.found === false ? '<p class="set-note warn">leo could not find this checkbox in the note any more. Open the note to check it.</p>' : ''}
    <label class="set-row cal-peek-done"><span class="set-label">${day ? `Done on ${esc(dayName)}` : 'Done'}</span><input type="checkbox" class="cal-check" data-action="task-done" data-id="${esc(task.id)}"${day ? ` data-on="${esc(day)}"` : ''}${doneNow ? ' checked' : ''}></label>
    ${repeating ? `<div class="set-row"><span class="set-label">Change</span>${segmented({ label: 'Which days change', action: 'cal-scope', key: 'cal-scope', current: scope, choices: [['one', 'This one'], ['all', 'Every time']] })}</div>` : ''}
    <div class="set-row column cal-peek-when"><span class="set-label">When</span><span class="cal-peek-times">
      <input class="set-input" type="date" id="cal-peek-date" aria-label="Date" value="${esc(slot.date || '')}">
      <input class="set-input" type="time" id="cal-peek-start" aria-label="Starts" value="${esc(calHM(slot.time) || '')}"${slot.date ? '' : ' disabled'}>
      ${leo ? `<span class="hint">to</span><input class="set-input" type="time" id="cal-peek-end" aria-label="Ends" value="${esc(calHM(slot.end) || '')}"${slot.time ? '' : ' disabled'}>` : ''}
    </span></div>
    ${leo ? `<div class="set-row column task-repeat" id="cal-peek-repeat"><span class="set-label">Repeat</span>${repeatBubbles(task.repeat || [])}<span class="hint repeat-hint">${esc(repeatHint(task.repeat || []))}</span></div>` : ''}
    <div class="set-row column"><span class="set-label">Type</span><div class="chips task-kinds" id="cal-peek-kinds">${KIND_ORDER.map((k) => `<button type="button" class="chip pick-chip${task.kind === k ? ' on' : ''}" data-kind="${k}" aria-pressed="${task.kind === k}">${esc(KIND_LABEL[k])}</button>`).join('')}</div></div>
    <textarea class="cal-peek-about" id="cal-peek-about" rows="4" maxlength="4000" placeholder="Add a description…" aria-label="Description">${esc(task.description || '')}</textarea>
    <div class="set-row"><span class="set-label">Note</span><span class="set-value">${task.note ? `<a href="${noteHash(task.note.id)}">${esc(task.note.title || 'Untitled')}</a>` : 'Added in leo'}</span></div>
    <div class="set-row"><span class="set-label">Google</span><span class="set-value" id="cal-peek-google">${esc(calPeekGoogle(task))}</span></div>
    ${repeating && Object.keys(task.moved || {}).length ? '<p class="hint">Google gets the weekly days; days you moved here stay in leo and the .ics file.</p>' : ''}
    <div class="buttons task-acts" id="cal-peek-acts">${calPeekActs(task)}</div>`;
  const saved = $('#cal-peek-saved', side);
  const said = (text) => { if (saved) saved.textContent = text; };
  const patch = async (body) => {
    said('Saving…');
    try {
      const out = await api(`/api/tasks/${enc(task.id)}`, { method: 'PATCH', body });
      if (out && out.id) Object.assign(task, out);
      else Object.assign(task, body);
      said('Saved');
      tasksWin.revision++;
      drawCalendar();
      loadTasksWindow().catch(() => {});
      return true;
    } catch (e) {
      said('');
      if (e instanceof Locked) throw e;
      toast(e.message, { bad: true });
      return false;
    }
  };
  const title = $('#cal-peek-title', side);
  const fit = () => { title.style.height = 'auto'; title.style.height = `${title.scrollHeight}px`; };
  fit();
  title.addEventListener('input', fit);
  title.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && !e.isComposing) { e.preventDefault(); title.blur(); }
    if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); title.value = task.title; fit(); title.blur(); }
  });
  title.addEventListener('blur', () => {
    const text = title.value.trim();
    if (!leo || !text || text === task.title) { title.value = task.title; fit(); return; }
    patch({ title: text }).catch(fail);
  });
  const about = $('#cal-peek-about', side);
  const aboutField = autosaveField({
    input: about,
    status: said,
    value: () => task.description || '',
    save: (text) => patch({ description: text }),
  });
  if (cal.peek) cal.peek.flush = aboutField.flush;
  for (const b of side.querySelectorAll('[data-cal-scope]')) {
    b.addEventListener('click', () => {
      if (cal.peek) cal.peek.scope = b.dataset.calScope;
      for (const o of side.querySelectorAll('[data-cal-scope]')) {
        o.classList.toggle('on', o === b);
        o.setAttribute('aria-pressed', String(o === b));
      }
      syncSegmented(b.closest('.segmented'));
    });
  }
  const when = () => {
    const date = $('#cal-peek-date', side).value || null;
    const startBox = $('#cal-peek-start', side);
    const time = date ? startBox.value || null : null;
    const endBox = $('#cal-peek-end', side);
    const end = time && endBox ? endBox.value || null : null;
    startBox.disabled = !date;
    if (endBox) endBox.disabled = !time;
    const next = { date, time, end };
    if (!date) return repeating ? toast('A repeating task needs a day.', { bad: true }) : calSave(task, task, next, 'one');
    const occurrence = repeating ? { ...task, ...slot, on: day, date: slot.date } : task;
    return calSave(task, occurrence, next, repeating ? (cal.peek && cal.peek.scope) || 'one' : 'one');
  };
  for (const sel of ['#cal-peek-date', '#cal-peek-start', '#cal-peek-end']) $(sel, side)?.addEventListener('change', () => Promise.resolve(when()).catch(fail));
  const repeatBox = $('#cal-peek-repeat', side);
  if (repeatBox) repeatPicker(repeatBox, task.repeat || [], (days) => patch({ repeat: days }).catch(fail));
  let kindLatest = 0;
  let kindSaved = task.kind;
  const pickKind = (on) => {
    if (!on) return;
    for (const c of side.querySelectorAll('[data-kind]')) {
      c.classList.toggle('on', c === on);
      c.setAttribute('aria-pressed', String(c === on));
    }
  };
  $('#cal-peek-kinds', side).addEventListener('click', (e) => {
    const chip = e.target.closest('[data-kind]');
    if (!chip || chip.classList.contains('on')) return;
    const mine = ++kindLatest;
    pickKind(chip);
    const back = () => { if (mine === kindLatest) pickKind(side.querySelector(`[data-kind="${CSS.escape(kindSaved || '')}"]`)); };
    patch({ kind: chip.dataset.kind }).then((ok) => (ok ? (kindSaved = chip.dataset.kind) : back()), (e) => {
      back();
      fail(e);
    });
  });
}

function taskDoneOn(task, on) {
  return on ? (task.done_on || []).includes(on) : Boolean(task.done);
}

function applyTaskDone(task, done, on) {
  if (task && on) {
    task.done_on = (task.done_on || []).filter((d) => d !== on);
    if (done) task.done_on = [...task.done_on, on].sort();
  } else if (task) task.done = done;
}

function markTaskDone(id, done, on) {
  tasksWin.revision++;
  applyTaskDone(calFind(id), done, on);
  finishTaskInWindow(id, done, on);
  drawCalendar();
  tickTaskBoxes(id, done, on);
}

function tickTaskBoxes(id, done, on) {
  for (const box of document.querySelectorAll(`.cal-check[data-id="${CSS.escape(id)}"]${on ? `[data-on="${CSS.escape(on)}"]` : ''}`)) box.checked = done;
}

const TICK_MS = 160;
const tickedAt = new Map();
const tickKey = (id, on) => `${id} ${on || ''}`;

function tickWrite(id, on) {
  const ago = performance.now() - (tickedAt.get(tickKey(id, on)) ?? -Infinity);
  return ago < TICK_MS ? ` data-writing style="--tick-at: ${-Math.round(ago)}ms"` : '';
}

async function setTaskDone(id, done, on = null) {
  const nowAt = performance.now();
  for (const [key, at] of tickedAt) if (nowAt - at >= TICK_MS) tickedAt.delete(key);
  if (done) tickedAt.set(tickKey(id, on), nowAt);
  const before = calFind(id);
  const was = before && (on ? before.done_on : before.done);
  markTaskDone(id, done, on);
  try {
    await api(`/api/tasks/${enc(id)}`, { method: 'PATCH', body: on ? { done, on } : { done } });
  } catch (e) {
    const task = calFind(id);
    if (task && on) task.done_on = was;
    else if (task) task.done = was;
    tasksWin.revision++;
    finishTaskInWindow(id, false, on);
    drawCalendar();
    tickTaskBoxes(id, task ? taskDoneOn(task, on) : !done, on);
    if (e instanceof Locked) throw e;
    toast(e.message, { bad: true });
    return loadSchedule(seq);
  }
  const task = calFind(id);
  tasksWin.revision++;
  if (task && taskDoneOn(task, on) !== done) {
    applyTaskDone(task, done, on);
    drawTasksWindow();
    drawCalendar();
    tickTaskBoxes(id, done, on);
  }
  if (done) toast(`Marked “${task?.title || 'Untitled'}” done`, { action: 'Undo', run: () => setTaskDone(id, false, on) });
  if (state.view === 'note' && task && task.note && state.session && state.session.note.id === task.note.id && !saving.unsaved()) showNote(task.note.id);
}

function pickerBody(box, value, { meeting = false, onChange = () => {} } = {}) {
  const pick = { date: value.date || null, time: value.time ? value.time.slice(0, 5) : '', end: value.end ? value.end.slice(0, 5) : '', month: monthOf(value.date || S.dayKey(new Date())) };
  const today = new Date();
  const draw = () => {
    const grid = S.monthGrid(pick.month.getFullYear(), pick.month.getMonth(), S.weekStart(calLocale()));
    const names = grid.slice(0, 7).map((c) => S.fromKey(c.key).toLocaleDateString(calLocale(), { weekday: 'narrow' }));
    const chip = (attrs, label, on) => `<button type="button" class="chip pick-chip${on ? ' on' : ''}" ${attrs} aria-pressed="${on}">${esc(label)}</button>`;
    const quick = [...S.quickDates(today).map((q) => chip(`data-pick-day="${q.key}"`, q.label, pick.date === q.key)), chip('data-pick-day=""', 'No date', !pick.date)].join('');
    const times = [['', 'All day'], ...[['09:00', 9], ['12:00', 12], ['15:00', 15], ['18:00', 18]].map(([t, h]) => [t, calHour(h * 60)]), ['23:59', 'End of day']];
    box.innerHTML = `<div class="pick-quick">${quick}</div>
      <div class="pick-month"><button type="button" class="icon-btn cal-arrow" data-pick-month="-1" aria-label="Previous month">${ICON.back}</button><span>${esc(pick.month.toLocaleDateString(calLocale(), { month: 'long', year: 'numeric' }))}</span><button type="button" class="icon-btn cal-arrow next" data-pick-month="1" aria-label="Next month">${ICON.back}</button></div>
      <div class="sched-grid small">${names.map((n) => `<span class="sched-wd">${esc(n)}</span>`).join('')}${grid.map((c) => `<button type="button" class="sched-day${c.inMonth ? '' : ' out'}${c.key === S.dayKey(today) ? ' today' : ''}${c.key === pick.date ? ' on' : ''}" data-pick-day="${c.key}"><span class="sched-n">${c.day}</span></button>`).join('')}</div>
      ${pick.date ? `<div class="pick-times"><span class="set-label">Time</span>${times.map(([t, label]) => chip(`data-pick-time="${t}"`, label, pick.time === t)).join('')}<input class="set-input pick-time" type="time" value="${esc(pick.time)}" aria-label="Time">${meeting && pick.time ? `<label class="pick-end">Ends <input class="set-input pick-end-time" type="time" value="${esc(pick.end)}"></label>` : ''}</div>` : ''}`;
  };
  box.addEventListener('click', (e) => {
    const day = e.target.closest('[data-pick-day]');
    const time = e.target.closest('[data-pick-time]');
    const month = e.target.closest('[data-pick-month]');
    if (day) {
      pick.date = day.dataset.pickDay || null;
      if (pick.date) pick.month = monthOf(pick.date);
      if (!pick.date) pick.time = '';
    } else if (time) {
      pick.time = time.dataset.pickTime;
      if (!pick.time) pick.end = '';
    } else if (month) {
      pick.month = new Date(pick.month.getFullYear(), pick.month.getMonth() + Number(month.dataset.pickMonth), 1);
    } else return;
    draw();
    onChange(pick);
  });
  box.addEventListener('change', (e) => {
    if (e.target.classList.contains('pick-time')) pick.time = e.target.value;
    else if (e.target.classList.contains('pick-end-time')) pick.end = e.target.value;
    else return;
    draw();
    onChange(pick);
  });
  draw();
  return pick;
}

function dateSheet(id) {
  const task = calFind(id);
  if (!task) return;
  const box = sheet(`<h3>${task.date ? 'Change the date' : 'Set a date'}</h3><p class="hint">${esc(task.title)}</p><div class="picker-box" id="pick-box"></div>
    <div class="buttons"><button type="button" class="btn plain" data-action="close">Cancel</button><button type="button" class="btn primary" id="pick-save">Save</button></div>`);
  const pick = pickerBody($('#pick-box', box), task, { meeting: task.kind === 'meeting' });
  $('#pick-save', box).addEventListener('click', () => saveDate(task, pick, false));
}

async function saveDate(task, pick, confirm) {
  const body = { date: pick.date || null, time: pick.time || null, end: pick.end || null };
  if (confirm) body.confirm = true;
  try {
    await api(`/api/tasks/${enc(task.id)}`, { method: 'PATCH', body });
  } catch (e) {
    if (e instanceof Locked) throw e;
    if (e.status === 409 && !confirm) {
      const preview = e.data && e.data.preview;
      if (preview) {
        const box = sheet(`<h3>Check the change</h3><p>leo could not find the old date in this task, so it adds the new one after it.</p>
          <div class="sched-preview"><span class="set-label">Before</span><p>${esc(preview.before)}</p><span class="set-label">After</span><p>${esc(preview.after)}</p></div>
          <div class="buttons"><button type="button" class="btn plain" data-action="close">Cancel</button><button type="button" class="btn primary" id="pick-confirm">Change it</button></div>`);
        $('#pick-confirm', box).addEventListener('click', () => saveDate(task, pick, true));
        return;
      }
    }
    toast(e.message, { bad: true });
    return;
  }
  closeSheet();
  const named = `“${task.title || 'Untitled'}”`;
  const day = pick.date && S.fromKey(pick.date).toLocaleDateString(calLocale(), { weekday: 'short', month: 'short', day: 'numeric' });
  toast(!pick.date ? 'Date cleared' : pick.date === task.date ? `Saved ${named}` : task.date ? `Moved ${named} to ${day}` : `Set ${named} for ${day}`);
  loadTasksWindow().catch(() => {});
  if (state.view === 'calendar') {
    if (pick.date) cal.selected = pick.date;
    await loadSchedule(seq);
  }
}

function newTaskSheet(day = null) {
  const box = sheet(`<h3>New task</h3>
    <div class="task-new">
      <div class="task-left">
        <input class="set-input task-title" id="task-title" maxlength="300" placeholder="What needs doing?" autocomplete="off" aria-label="Title">
        <label class="set-row column task-about"><span class="set-label">Description <span class="hint">optional</span></span><textarea class="set-input" id="task-description" rows="3" maxlength="4000" placeholder="Add a description…"></textarea></label>
        <div class="set-row column"><span class="set-label">Type</span><div class="chips task-kinds" id="task-kinds">${KIND_ORDER.map((k) => `<button type="button" class="chip pick-chip${k === 'other' ? ' on' : ''}" data-kind="${k}" aria-pressed="${k === 'other'}">${esc(KIND_LABEL[k])}</button>`).join('')}</div></div>
        <div class="set-row column task-repeat" id="task-repeat"><span class="set-label">Repeat</span>${repeatBubbles([])}<span class="hint repeat-hint">${esc(repeatHint([]))}</span></div>
      </div>
      <div class="picker-box" id="pick-box"></div>
    </div>
    <div class="buttons"><button type="button" class="btn plain" data-action="close">Cancel</button><button type="button" class="btn primary" id="task-add">Add task</button></div>`);
  let kind = 'other';
  let repeat = [];
  const pick = pickerBody($('#pick-box', box), { date: day }, { meeting: false });
  repeatPicker($('#task-repeat', box), [], (days) => { repeat = days; });
  $('#task-kinds', box).addEventListener('click', (e) => {
    const chip = e.target.closest('[data-kind]');
    if (!chip) return;
    kind = chip.dataset.kind;
    for (const c of box.querySelectorAll('[data-kind]')) {
      c.classList.toggle('on', c === chip);
      c.setAttribute('aria-pressed', String(c === chip));
    }
  });
  const title = $('#task-title', box);
  const add = $('#task-add', box);
  const save = async () => {
    const text = title.value.trim();
    if (!text) {
      title.focus();
      return toast('Give the task a title.', { bad: true });
    }
    if (add.disabled) return;
    add.disabled = true;
    try {
      const made = await api('/api/tasks', { method: 'POST', body: { title: text, description: $('#task-description', box).value.trim(), date: pick.date, time: pick.time || null, kind, repeat } });
      inkLand(made.id);
      closeSheet();
      toast(`Added “${made.title}”`);
      loadTasksWindow().catch(() => {});
      if (state.view === 'calendar') {
        if (made.date && !S.repeats(made)) cal.selected = made.date;
        await loadSchedule(seq);
      }
    } catch (e) {
      add.disabled = false;
      if (e instanceof Locked) throw e;
      toast(e.message, { bad: true });
    }
  };
  add.addEventListener('click', save);
  title.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && !e.isComposing) {
      e.preventDefault();
      save();
    }
  });
  title.focus();
}

let taskField = null;
const autosaves = new Set();

function autosaveField({ input, status, value, save, delay = 600 }) {
  let timer = 0;
  let busy = null;
  const field = {
    text: () => input.value.trim(),
    pending: () => Boolean(timer || busy) || input.value.trim() !== value(),
    async flush() {
      clearTimeout(timer);
      timer = 0;
      if (busy) {
        await busy;
        return field.flush();
      }
      const text = input.value.trim();
      if (text === value()) return true;
      status('Saving…');
      busy = save(text);
      let ok = false;
      try {
        ok = (await busy) !== false;
      } finally {
        busy = null;
      }
      if (ok) {
        status('Saved');
        if (input.value.trim() !== value()) timer = setTimeout(field.flush, delay);
      } else {
        status('Not saved, will retry');
        timer = setTimeout(field.flush, 3000);
      }
      return ok;
    },
  };
  input.addEventListener('input', () => {
    status('');
    clearTimeout(timer);
    timer = setTimeout(field.flush, delay);
  });
  input.addEventListener('blur', () => field.flush().catch(fail));
  autosaves.add(field);
  return field;
}

function pendingAutosaves() {
  for (const f of autosaves) if (!f.pending()) autosaves.delete(f);
  return [...autosaves];
}

function taskSheet(id, on = null) {
  const task = calFind(id);
  if (!task) return;
  const repeating = S.repeats(task);
  const day = repeating ? on || (state.view === 'calendar' ? cal.selected : S.dayKey(new Date())) : null;
  const doneNow = taskDoneOn(task, day);
  const dayName = day ? S.fromKey(day).toLocaleDateString(calLocale(), { weekday: 'short', month: 'short', day: 'numeric' }) : '';
  const onGoogle = (cal.data && (cal.data.events || []).find((e) => e.task === task.id)) || null;
  const google = onGoogle ? 'On Google Calendar' : task.sent_at ? 'Opened in Google Calendar' : 'Not added';
  const when = task.date
    ? `${S.fromKey(task.date).toLocaleDateString(calLocale(), { weekday: 'short', month: 'short', day: 'numeric', year: 'numeric' })}${task.time ? ` · ${S.timeLabel(task.time, calLocale())}` : ''}`
    : 'No date';
  const moved = onGoogle && task.date && !onGoogle.all_day && S.dayKey(new Date(onGoogle.start)) !== task.date && task.time;
  const box = sheet(`<h3 class="task-sheet-title">${esc(task.title)}</h3>
    ${task.found === false ? '<p class="set-note warn">leo could not find this checkbox in the note any more. Open the note to check it.</p>' : ''}
    <div class="set-row column"><label class="set-label" for="task-description">Description <span class="hint">optional</span><span class="hint task-saved" id="task-description-state" role="status"></span></label><textarea class="set-input" id="task-description" rows="3" maxlength="4000" placeholder="Add a description…">${esc(task.description || '')}</textarea></div>
    <div class="set-row"><span class="set-label">${repeating ? 'Starts' : 'Date'}</span><span class="set-value">${esc(when)}<button type="button" class="btn plain sm" data-action="task-date" data-id="${esc(task.id)}">${task.date ? 'Change' : 'Set date'}</button></span></div>
    ${task.source === 'leo' ? `<div class="set-row column task-repeat" id="task-repeat"><span class="set-label">Repeat</span>${repeatBubbles(task.repeat || [])}<span class="hint repeat-hint">${esc(repeatHint(task.repeat || []))}</span></div>` : ''}
    <div class="set-row column"><span class="set-label">Type</span><div class="chips task-kinds" id="task-kinds">${KIND_ORDER.map((k) => `<button type="button" class="chip pick-chip${task.kind === k ? ' on' : ''}" data-kind="${k}" aria-pressed="${task.kind === k}">${esc(KIND_LABEL[k])}</button>`).join('')}</div></div>
    <div class="set-row"><span class="set-label">Note</span><span class="set-value">${task.note ? `<a href="${noteHash(task.note.id)}">${esc(task.note.title || 'Untitled')}</a>` : 'Added in leo'}</span></div>
    <div class="set-row"><span class="set-label">Google</span><span class="set-value">${esc(google)}</span></div>
    ${moved ? `<p class="set-note">The Google event is on another day. <a href="${esc(S.googleDay(S.dayKey(new Date(onGoogle.start))))}" target="_blank" rel="noopener">Open that day in Google Calendar</a></p>` : ''}
    <div class="buttons task-acts">
      ${task.date && !onGoogle ? `<button type="button" class="icon-btn" aria-label="Add to Google Calendar" title="Add to Google Calendar" data-action="task-google" data-id="${esc(task.id)}">${ICON.calendar}</button>` : ''}
      ${!task.date ? `<button type="button" class="btn primary" data-action="task-date" data-id="${esc(task.id)}" data-then="google">Set a date to add it to Google Calendar</button>` : ''}
      ${task.date ? `<a class="icon-btn" href="/api/tasks/${enc(task.id)}/ics" download aria-label="Download .ics" title="Download .ics">${ICON.download}</a>` : ''}
      <button type="button" class="btn plain" data-action="task-done" data-id="${esc(task.id)}"${day ? ` data-on="${esc(day)}"` : ''} data-set="${doneNow ? '0' : '1'}">${doneNow ? (day ? `Not done on ${esc(dayName)}` : 'Not done') : (day ? `Done for ${esc(dayName)}` : 'Done')}</button>
      ${task.source === 'leo' ? `<button type="button" class="icon-btn danger" aria-label="Delete" title="Delete" data-action="task-delete" data-id="${esc(task.id)}">${ICON.trash}</button>` : task.promoted ? `<button type="button" class="icon-btn" aria-label="Stop tracking" title="Stop tracking" data-action="task-delete" data-id="${esc(task.id)}">${ICON.close}</button>` : ''}
    </div>`);
  const description = $('#task-description', box);
  const descState = $('#task-description-state', box);
  const opened = task.description || '';
  let descSaved = false;
  const earlier = taskField && taskField.id === id && taskField.field.pending() ? taskField : null;
  if (earlier) description.value = earlier.field.text();
  const field = autosaveField({
    input: description,
    status: (text) => { if (descState.isConnected) descState.textContent = text; },
    value: () => task.description || '',
    save: async (text) => {
      try {
        const saved = await api(`/api/tasks/${enc(task.id)}`, { method: 'PATCH', body: { description: text } });
        task.description = saved.description;
        if (!descSaved) {
          descSaved = true;
          toast(`Saved the description of “${task.title || 'Untitled'}”`, { action: 'Undo', run: async () => {
            const undone = await api(`/api/tasks/${enc(task.id)}`, { method: 'PATCH', body: { description: opened } });
            task.description = undone.description;
            if (box.isConnected) {
              description.value = opened;
              descState.textContent = '';
            }
            loadTasksWindow().catch(() => {});
          } });
        }
        loadTasksWindow().catch(() => {});
        return true;
      } catch (err) {
        if (err instanceof Locked) throw err;
        toast(err.message, { bad: true });
        return false;
      }
    },
  });
  taskField = { id, field };
  if (earlier) earlier.field.flush().then(field.flush).catch(fail);
  const repeatBox = $('#task-repeat', box);
  if (repeatBox) {
    repeatPicker(repeatBox, task.repeat || [], async (days) => {
      try {
        const saved = await api(`/api/tasks/${enc(task.id)}`, { method: 'PATCH', body: { repeat: days } });
        Object.assign(task, saved && saved.id ? saved : { repeat: days });
        if (state.view === 'calendar') drawCalendar();
        drawTasksWindow();
      } catch (err) {
        if (err instanceof Locked) throw err;
        toast(err.message, { bad: true });
      }
    });
  }
  let kindLatest = 0;
  let kindSaved = task.kind;
  $('#task-kinds', box).addEventListener('click', async (e) => {
    const chip = e.target.closest('[data-kind]');
    if (!chip || chip.classList.contains('on')) return;
    const mine = ++kindLatest;
    const pick = (on) => {
      if (!on) return;
      for (const c of box.querySelectorAll('[data-kind]')) {
        c.classList.toggle('on', c === on);
        c.setAttribute('aria-pressed', String(c === on));
      }
    };
    task.kind = chip.dataset.kind;
    pick(chip);
    if (state.view === 'calendar') drawCalendar();
    try {
      await api(`/api/tasks/${enc(task.id)}`, { method: 'PATCH', body: { kind: chip.dataset.kind } });
      kindSaved = chip.dataset.kind;
    } catch (err) {
      if (mine === kindLatest) {
        task.kind = kindSaved;
        pick(box.querySelector(`[data-kind="${CSS.escape(kindSaved || '')}"]`));
        if (state.view === 'calendar') drawCalendar();
      }
      if (err instanceof Locked) throw err;
      toast(err.message, { bad: true });
    }
  });
}

async function addToGoogle(id) {
  const task = calFind(id);
  if (!task) return;
  if (!task.date) return dateSheet(id);
  const link = S.googleLink(task, calZone());
  window.open(link, '_blank', 'noopener');
  try {
    await api(`/api/tasks/${enc(id)}`, { method: 'PATCH', body: { sent: true } });
    task.sent_at = new Date().toISOString();
  } catch (e) {
    if (e instanceof Locked) throw e;
  }
  closeSheet();
  calPeekRefresh(id);
  toast('Opened in Google Calendar. Press Save there; it shows here as one item once Google updates the calendar link.');
  if (state.view === 'calendar') drawCalendar();
}

async function deleteTask(id) {
  const task = calFind(id);
  try {
    const gone = await api(`/api/tasks/${enc(id)}`, { method: 'DELETE' });
    closeSheet();
    if (cal.peek && cal.peek.id === id) calClosePeek();
    if (gone && task && task.source === 'leo') {
      toast(`Deleted “${gone.title}”`, {
        action: 'Undo',
        run: async () => {
          await api('/api/tasks/import', { method: 'POST', body: { tasks: [gone] } });
          await loadSchedule(seq);
        },
      });
    } else toast('No longer tracked as a task. The checkbox stays in the note.');
    await loadSchedule(seq);
  } catch (e) {
    if (e instanceof Locked) throw e;
    toast(e.message, { bad: true });
  }
}

async function noteTasksSheet() {
  const s = state.view === 'note' ? state.session : null;
  if (!s || !s.note.id) return;
  const ready = await ready_();
  if (!ready) return;
  let listed = [];
  try {
    listed = (await api('/api/tasks')).tasks.filter((t) => t.note && t.note.id === s.note.id);
  } catch (e) {
    if (e instanceof Locked) throw e;
  }
  const body = s.doc ? s.doc.source() : '';
  const lines = body.split('\n');
  const task = new RegExp(String.raw`^\s*(?:${md.MARKER})\s{1,4}\[(?<mark>[ xX])\]\s(?<text>.+)$`);
  const boxes = lines
    .map((line, n) => ({ n, m: line.match(task)?.groups }))
    .filter((b) => b.m && b.m.text.trim());
  const rows = boxes.slice(0, 200).map((b) => {
    const tracked = listed.find((t) => t.line === b.n);
    return `<div class="set-row"><span class="grow">${b.m.mark !== ' ' ? '<s>' : ''}${esc(b.m.text.trim())}${b.m.mark !== ' ' ? '</s>' : ''}</span>${tracked ? `<button type="button" class="btn plain sm" data-action="task-open-here" data-id="${esc(tracked.id)}">In your tasks</button>` : `<button type="button" class="btn sm" data-action="task-promote" data-line="${b.n}">Add to tasks</button>`}</div>`;
  }).join('');
  sheet(`<h3>Tasks in this note</h3>
    <p class="hint">Checkboxes under an Action items heading are tasks already. Add any other checkbox to show it on the Calendar.</p>
    ${rows || '<p class="hint">This note has no checkboxes yet.</p>'}
    <div class="buttons"><button type="button" class="btn plain" data-action="close">Close</button><button type="button" class="btn primary" data-action="calendar">Open the Calendar</button></div>`);
}

async function ready_() {
  try {
    return await ready();
  } catch (e) {
    if (e instanceof Locked) throw e;
    toast(e.message, { bad: true });
    return null;
  }
}

async function promoteTask(line) {
  const s = state.view === 'note' ? state.session : null;
  if (!s || !s.note.id) return;
  try {
    const task = await api('/api/tasks/promote', { method: 'POST', body: { note: s.note.id, line: Number(line) } });
    toast(`Added “${task?.title || 'Untitled'}” to your tasks`, { action: 'Calendar', run: () => go('#/calendar') });
    await noteTasksSheet();
  } catch (e) {
    if (e instanceof Locked) throw e;
    toast(e.status === 400 ? 'That line is not a checkbox any more.' : e.message, { bad: true });
  }
}

async function openTaskAnywhere(id, on = null) {
  if (!cal.data || !calFind(id)) {
    try {
      cal.data = { ...(cal.data || {}), tasks: (await api('/api/tasks')).tasks, events: (cal.data && cal.data.events) || [] };
    } catch (e) {
      if (e instanceof Locked) throw e;
      return;
    }
  }
  taskSheet(id, on);
}

var tasksWin = { timer: 0, tasks: null, events: null, eventsAt: 0, finished: new Map(), revision: 0, load: 0 };
const TASKS_AHEAD_DAYS = 7;
const TASK_GROUP_NAMES = { Overdue: 'Earlier' };
const TASK_DONE_MS = 1200;
const TASKS_LEAST_ROOM = 150;
const MOST_TASK_FEEDBACK = 80;

function dock() {
  let box = document.getElementById('dock');
  if (!box) {
    box = document.createElement('div');
    box.id = 'dock';
    box.className = 'dock';
    document.body.appendChild(box);
  }
  return box;
}

const tasksBox = document.createElement('aside');
tasksBox.className = 'tasks-window';
tasksBox.id = 'tasks-window';
tasksBox.hidden = true;
tasksBox.tabIndex = 0;
tasksBox.setAttribute('aria-label', 'Tasks');
dock().prepend(tasksBox);
tasksBox.addEventListener('mousedown', (e) => {
  if (!e.target.closest('input, textarea, select, [contenteditable]')) e.preventDefault();
});
const TASKS_HOLD_MS = 300;
let tasksOpened = { at: 0, x: 0, y: 0 };
tasksBox.addEventListener('pointerenter', (e) => { tasksOpened = { at: performance.now(), x: e.clientX, y: e.clientY }; });
tasksBox.addEventListener('click', (e) => {
  if (performance.now() - tasksOpened.at < TASKS_HOLD_MS && Math.hypot(e.clientX - tasksOpened.x, e.clientY - tasksOpened.y) < 4 && !e.target.closest('.tasks-head')) {
    e.preventDefault();
    e.stopImmediatePropagation();
  }
}, true);

function tasksGroups(tasks, finishing = false) {
  const today = S.dayKey(new Date());
  const ahead = S.dayKey(S.addDays(new Date(), TASKS_AHEAD_DAYS));
  const groups = [['Overdue', []], ['Today', []], ['Coming up', []], ['No date', []]];
  for (const task of tasks) {
    const held = finishing && tasksWin.finished.get(task.id);
    const t = held ? held.task : S.repeats(task) ? S.nextOpen(task, today, ahead) : task;
    if (!t || (t.done && !held)) continue;
    if (!t.date) groups[3][1].push(t);
    else if (t.date < today) groups[0][1].push(t);
    else if (t.date === today) groups[1][1].push(t);
    else if (t.date <= ahead) groups[2][1].push(t);
  }
  return groups.filter(([, list]) => list.length);
}

function finishTaskInWindow(id, done, on) {
  const held = tasksWin.finished.get(id);
  if (held) clearTimeout(held.timer);
  tasksWin.finished.delete(id);
  const task = calFind(id);
  const row = tasksHost().querySelector(`.tasks-row[data-task="${CSS.escape(id)}"]`);
  if (!done || !task || !row) { drawTasksWindow(); return; }
  if (tasksAtHome() && (on || task.date) === S.dayKey(new Date())) {
    drawTasksWindow();
    pulse(tasksHost().querySelector(`.sched-check[data-id="${CSS.escape(id)}"]`), { lift: 1.18 });
    return;
  }
  if (tasksWin.finished.size >= MOST_TASK_FEEDBACK) {
    const oldest = tasksWin.finished.keys().next().value;
    clearTimeout(tasksWin.finished.get(oldest).timer);
    tasksWin.finished.delete(oldest);
  }
  const entry = { task: { ...task, done: true, date: on || task.date, on }, timer: 0 };
  tasksWin.finished.set(id, entry);
  drawTasksWindow();
  const check = tasksHost().querySelector(`.sched-check[data-id="${CSS.escape(id)}"]`);
  pulse(check, { lift: 1.18 });
  entry.timer = setTimeout(() => {
    if (tasksWin.finished.get(id) !== entry) return;
    tasksWin.finished.delete(id);
    const row = tasksHost().querySelector(`.tasks-row[data-task="${CSS.escape(id)}"]`);
    if (row) {
      const height = row.getBoundingClientRect().height;
      row.style.minHeight = '0';
      slideHeight(row, height, 0, drawTasksWindow);
    } else drawTasksWindow();
  }, TASK_DONE_MS);
}

function tasksDue() {
  const tasks = tasksWin.tasks || (cal.data && cal.data.tasks) || [];
  const groups = new Map(tasksGroups(tasks));
  const late = (groups.get('Overdue') || []).length;
  return { late, due: late + (groups.get('Today') || []).length };
}

function tasksDueBadge() {
  const { late, due } = tasksDue();
  if (!due) return '';
  return `<span class="due-badge${late ? ' late' : ''}" aria-label="${esc(late ? `${due} due, ${late} from earlier` : `${due} due today`)}">${due} due</span>`;
}

const tasksAtHome = () => state.view === 'workspace' && /^(?:#\/?)?$/.test(location.hash);
const tasksHost = () => (tasksAtHome() && document.getElementById('home-today')) || tasksBox;

function tasksRow(t) {
  return `<div class="tasks-row${t.done ? ' done' : ''}" data-task="${esc(t.id)}" style="--tone: var(${toneOfTask(t)})">
    <input type="checkbox" class="sched-check" data-action="task-done" data-id="${esc(t.id)}"${t.on ? ` data-on="${esc(t.on)}"` : ''}${t.done ? ` checked${tickWrite(t.id, t.on)}` : ''} aria-label="Done: ${esc(t.title)}">
    <button type="button" class="tasks-title" data-action="task-open-here" data-id="${esc(t.id)}"${t.on ? ` data-on="${esc(t.on)}"` : ''}>${esc(t.title)}</button>
    ${t.date ? `<span class="tasks-when" aria-hidden="true" title="${esc(S.dayName(t.date, calLocale()))}">${esc(t.date === S.dayKey(new Date()) && t.time ? S.timeLabel(t.time, calLocale()) : S.dayWord(t.date, new Date(), calLocale()))}</span><span class="said">${esc(S.dayName(t.date, calLocale()))}${t.date === S.dayKey(new Date()) && t.time ? `, ${esc(S.timeLabel(t.time, calLocale()))}` : ''}</span>` : `<button type="button" class="btn plain sm" data-action="task-date" data-id="${esc(t.id)}">Set date</button>`}
  </div>`;
}

function tasksRows(groups) {
  return groups.map(([name, list]) => `<div class="tasks-group${name === 'Overdue' ? ' late' : ''}">${TASK_GROUP_NAMES[name] || name}</div>${list.slice(0, name === 'No date' ? 5 : 20).map((t) => tasksRow(t)).join('')}`).join('');
}

function tasksFocus(host) {
  const active = host.contains(document.activeElement) ? document.activeElement : null;
  return active ? {action: active.dataset.action, id: active.dataset.id, on: active.dataset.on} : null;
}

function refocusTasks(host, focus) {
  if (!focus) return;
  const same = [...host.querySelectorAll('[data-action]')].find(el => el.dataset.action === focus.action && el.dataset.id === focus.id && el.dataset.on === focus.on);
  (same || host).focus({ preventScroll: true });
}

const tasksInked = (id) => document.querySelectorAll(`.tasks-row[data-task="${CSS.escape(id)}"]`);

const HOME_ROWS = 6;
const HOME_RECENT_WAIT = 20000;
let homeMidnight = 0;

function drawHomeToday() {
  const box = document.getElementById('home-today');
  if (!box) return;
  const now = new Date();
  const known = tasksWin.tasks || (cal.data && cal.data.tasks);
  const tasks = known || [];
  const { earlier, today } = S.homeDay(tasks, now, new Set(tasksWin.finished.keys()));
  const early = earlier.slice(0, Math.max(2, HOME_ROWS - today.length));
  const day = today.slice(0, HOME_ROWS - early.length);
  const more = earlier.length + today.length - early.length - day.length;
  const focus = tasksFocus(box);
  const said = `${today.filter((t) => t.done).length} of ${today.length} done`;
  box.hidden = !early.length && !day.length;
  box.innerHTML = `${early.length ? `<div class="tasks-group">Earlier</div>${early.map((t) => tasksRow(t)).join('')}` : ''}${day.length ? `<div class="tasks-group">Today<span class="home-tally" title="${said}">${homeTally(today)}<span class="home-tally-said">${said}</span></span></div>${day.map((t) => tasksRow(t)).join('')}` : ''}${more ? `<a class="home-more" href="#/calendar"><b>${more} more</b> in Calendar</a>` : ''}`;
  inkSettle(tasksInked);
  if (!box.hidden) refocusTasks(box, focus);
  const root = side.notes.get('');
  const notes = side.loaded && root && root.loaded && !side.tasks ? root.items.length + side.folders.length : null;
  const ready = known && (notes !== null || !sideActive() || (root && !root.loading));
  const parts = ready ? S.dateline({ tasks, events: tasksWin.events || [], now, notes, locale: calLocale() }) : [];
  $('#home-day').textContent = now.toLocaleDateString(calLocale(), { weekday: 'long', month: 'long', day: 'numeric' });
  $('#home-line').innerHTML = parts.map((part) => (typeof part === 'string' ? esc(part) : `<b>${esc(part.title)}</b>`)).join('');
  const first = ready && notes === 0 && !tasks.length;
  $('.home-foot').classList.toggle('first', Boolean(first));
  $('.home-foot').hidden = !first && sideActive();
  $('#home-foot').textContent = first ? `Notes you write appear ${sideActive() ? 'on the left' : 'in Folders'}.` : 'Open a note from Folders, or start a new one.';
  drawHomeWeek();
  drawHomeRecent();
  clearTimeout(homeMidnight);
  homeMidnight = setTimeout(() => { drawTasksWindow(); loadHomeWeek(); }, S.addDays(now, 1) - now + 1000);
}

function homeTally(today) {
  const done = today.filter((t) => t.done);
  const { marks, width } = S.tally(done.length);
  if (!marks.length) return '';
  const ago = performance.now() - Math.max(...done.map((t) => tickedAt.get(tickKey(t.id, t.on)) ?? -Infinity));
  return `<svg class="home-tally-marks" width="${width}" height="12" viewBox="0 0 ${width} 12" aria-hidden="true">${marks.map((m, i) => `<path${m.x ? ` transform="translate(${m.x} 0)"` : ''} d="${m.d}"${i === marks.length - 1 && ago < TICK_MS ? ` pathLength="1" class="inking" style="animation-delay: ${-Math.round(ago)}ms"` : ''}/>`).join('')}</svg>`;
}

const homeWeek = { events: null, load: 0, from: '', at: 0 };
const HOME_WEEK_FRESH = 10 * 60000;

async function loadHomeWeek() {
  if (!document.getElementById('home-week')) return;
  const days = S.daysOf('week', S.dayKey(new Date()), S.weekStart(calLocale()));
  const mine = ++homeWeek.load;
  homeWeek.from = days[0];
  homeWeek.at = Date.now();
  try {
    const data = await api(`/api/schedule?from=${enc(S.fromKey(days[0]).toISOString())}&to=${enc(S.addDays(S.fromKey(days[6]), 1).toISOString())}`);
    if (mine !== homeWeek.load) return;
    homeWeek.events = data.events || [];
  } catch (e) {
    return;
  }
  drawHomeWeek();
}

function drawHomeWeek() {
  const box = document.getElementById('home-week');
  if (!box) return;
  const today = S.dayKey(new Date());
  const days = S.daysOf('week', today, S.weekStart(calLocale()));
  if (homeWeek.from && homeWeek.from !== days[0]) loadHomeWeek();
  const plan = S.plan(tasksWin.tasks || (cal.data && cal.data.tasks) || [], homeWeek.events || [], today, { from: days[0], to: days[6] });
  const focus = tasksFocus(box);
  const held = focus && days.includes(focus.id) ? focus.id : today;
  const html = days.map((key) => {
    const d = S.fromKey(key);
    const name = d.toLocaleDateString(calLocale(), { weekday: 'long', month: 'long', day: 'numeric' });
    const busy = plan.byDay.has(key);
    const stop = key === held;
    return `<button type="button" class="home-wd${key < today ? ' past' : key === today ? ' today' : ''}" data-action="home-day" data-id="${key}" tabindex="${stop ? 0 : -1}" aria-label="${esc(busy ? `${name}, planned` : name)}" title="${esc(name)}"${key === today ? ' aria-current="date"' : ''}><b>${d.getDate()}${key === today ? calRing(key, true) : ''}</b>${busy ? '<i></i>' : ''}</button>`;
  }).join('');
  if (box.drawn === html) return;
  box.drawn = html;
  box.innerHTML = html;
  box.onkeydown = homeWeekKeys;
  if (focus) box.querySelector('[tabindex="0"]')?.focus({ preventScroll: true });
}

function homeWeekKeys(e) {
  const days = [...e.currentTarget.querySelectorAll('.home-wd')];
  const at = days.indexOf(document.activeElement);
  const to = { ArrowLeft: at - 1, ArrowRight: at + 1, Home: 0, End: days.length - 1 }[e.key];
  if (at < 0 || to === undefined || !days[to]) return;
  e.preventDefault();
  for (const day of days) day.tabIndex = day === days[to] ? 0 : -1;
  days[to].focus();
}

const homeRecent = { notes: null, busy: 0, again: false };

async function loadHomeRecent() {
  if (!document.getElementById('home-recent')) return;
  if (homeRecent.busy && Date.now() - homeRecent.busy < HOME_RECENT_WAIT) {
    homeRecent.again = true;
    return;
  }
  const mine = (homeRecent.busy = Date.now());
  try {
    const pinned = (await api('/api/notes?show=pinned&limit=1', { total: true })).total;
    const notes = await api(`/api/notes?sort=updated&brief=true&limit=${Math.min(pinned + 3, 1000)}`);
    if (homeRecent.busy !== mine) return;
    homeRecent.notes = (Array.isArray(notes) ? notes : []).filter((n) => typeof n.id === 'string').sort((a, b) => Date.parse(b.updated_at) - Date.parse(a.updated_at)).slice(0, 3);
  } catch (e) {
    if (!homeRecent.notes) homeRecent.notes = [];
  } finally {
    if (homeRecent.busy === mine) homeRecent.busy = 0;
  }
  drawHomeRecent();
  if (homeRecent.again) {
    homeRecent.again = false;
    loadHomeRecent();
  }
}

function drawHomeRecent() {
  const box = document.getElementById('home-recent');
  if (!box || !homeRecent.notes) return;
  const at = box.contains(document.activeElement) ? document.activeElement.getAttribute('href') : null;
  const html = homeRecent.notes.length ? `<div class="tasks-group">Recently written</div>${homeRecent.notes.map((n) => `<a class="tasks-row home-recent-row" href="${noteHash(n.id)}">${ICON.note}<span class="tasks-title">${esc(n.title || 'Untitled')}</span><span class="home-recent-line">${esc(n.summary || '')}</span><span class="tasks-when">${esc(rel(n.updated_at))}</span></a>`).join('')}` : '';
  box.hidden = !homeRecent.notes.length;
  if (box.drawn === html) return;
  box.drawn = html;
  box.innerHTML = html;
  if (at) box.querySelector(`[href="${CSS.escape(at)}"]`)?.focus({ preventScroll: true });
}

function drawTasksWindow() {
  const tasks = tasksWin.tasks || (cal.data && cal.data.tasks) || [];
  const groups = tasksGroups(tasks, true);
  const open = groups.reduce((n, [, list]) => n + list.length, 0);
  const home = tasksAtHome();
  if (home) drawHomeToday();
  // Calendar and Home list these tasks themselves, and on Boards the panel would cover the columns.
  const here = home || /^#\/(?:calendar|boards)(?:\/|$)/.test(location.hash);
  tasksBox.hidden = !open || here || state.view === 'locked';
  if (tasksBox.hidden) { fitTasksWindow(); return; }
  const late = groups.find(([name]) => name === 'Overdue');
  const focus = tasksFocus(tasksBox);
  const label = late ? `${late[1].length} from earlier · ${open} to do` : `${open} to do`;
  tasksBox.setAttribute('aria-label', `Tasks: ${label}`);
  tasksBox.innerHTML = `<div class="tasks-head">
      <div class="tasks-toggle"><span class="tasks-icon${late ? ' late' : ''}">${ICON.check}</span><span class="tasks-badge${late ? ' late' : ''}">${open > 99 ? '99+' : open}</span><span class="grow">Tasks <span class="tasks-count">${esc(label)}</span></span></div>
      <button type="button" class="icon-btn tasks-new" data-action="task-new" aria-label="New task" title="New task">${ICON.plus}</button>
    </div>
    <div class="tasks-body">${tasksRows(groups)}</div>
    <div class="tasks-foot"><button type="button" class="link-btn" data-action="calendar">Open calendar</button></div>`;
  inkSettle(tasksInked);
  refocusTasks(tasksBox, focus);
  fitTasksWindow();
}

function fitTasksWindow() {
  const first = app.querySelector('.cards > .card');
  const box = tasksBox.hidden ? null : dock().getBoundingClientRect();
  const card = first && box && box.width ? first.getBoundingClientRect() : null;
  const over = Boolean(card && card.left < box.right && card.top + scrollY < box.bottom);
  const rail = box && box.width ? [...app.querySelectorAll('.note-outline .outline-ticks, .note-outline summary')]
    .filter((el) => el.offsetParent)
    .map((el) => el.getBoundingClientRect())
    .filter((r) => r.right > box.left && r.left < box.right && r.bottom > 0) : [];
  let room = over ? Math.floor(box.bottom - card.bottom - scrollY - 12) : Infinity;
  if (rail.length) room = Math.min(room, Math.floor(box.bottom - Math.max(...rail.map((r) => r.bottom)) - 12));
  if (Number.isFinite(room)) tasksBox.style.setProperty('--tasks-room', `${Math.max(TASKS_LEAST_ROOM, room)}px`);
  else tasksBox.style.removeProperty('--tasks-room');
  if (over) document.body.style.setProperty('--dock-room', `${Math.ceil(innerHeight - dock().getBoundingClientRect().top + 16)}px`);
  else document.body.style.removeProperty('--dock-room');
}

new MutationObserver(fitTasksWindow).observe(app, { childList: true });
let railFit = 0;
new MutationObserver(() => {
  if (railFit || !(app.querySelector('.outline-ticks') || tasksBox.style.getPropertyValue('--tasks-room'))) return;
  railFit = requestAnimationFrame(() => { railFit = 0; fitTasksWindow(); });
}).observe(app, { childList: true, subtree: true });
window.addEventListener('resize', fitTasksWindow);

async function loadTasksWindow() {
  clearTimeout(tasksWin.timer);
  if (state.view === 'locked') return;
  const mine = ++tasksWin.load;
  tasksWin.stale = false;
  const revision = tasksWin.revision;
  try {
    const home = tasksAtHome();
    const now = new Date();
    const data = home
      ? await api(`/api/schedule?from=${enc(S.fromKey(S.dayKey(now)).toISOString())}&to=${enc(S.addDays(now, 8).toISOString())}`).catch((e) => { if (e instanceof Locked) throw e; return api('/api/tasks'); })
      : await api('/api/tasks');
    const tasks = data.tasks;
    if (mine !== tasksWin.load) return;
    if (data.events) {
      if (home && ((tasksWin.events && JSON.stringify(data.events) !== JSON.stringify(tasksWin.events)) || Date.now() - homeWeek.at > HOME_WEEK_FRESH)) loadHomeWeek();
      tasksWin.events = data.events;
      tasksWin.eventsAt = Date.now();
    }
    if (revision === tasksWin.revision) {
      tasksWin.tasks = tasks;
      if (cal.data) cal.data = { ...cal.data, tasks };
      else cal.data = { events: [], tasks };
    }
  } catch (e) {
    if (e instanceof Locked) return;
  }
  if (mine !== tasksWin.load) return;
  drawTasksWindow();
  tasksWin.timer = setTimeout(() => {
    if (appVisible()) loadTasksWindow().catch(() => {});
    else tasksWin.stale = true;
  }, CAL_POLL_MS);
}

setTimeout(() => loadTasksWindow().catch(() => {}), 0);
window.addEventListener('hashchange', () => drawTasksWindow());
const homeShown = () => { if (tasksAtHome()) drawTasksWindow(); };
window.addEventListener('focus', homeShown);
window.addEventListener('leo-visibility', homeShown);

const source = { note: null, name: null, seq: 0, returnTo: null };
const SOURCE_MIN = 260;
const SOURCE_KEY = 'leo-source-width';

const sourcePane = document.createElement('aside');
sourcePane.className = 'source-pane';
sourcePane.id = 'source-pane';
sourcePane.hidden = true;
sourcePane.setAttribute('aria-label', 'Original file');
document.body.appendChild(sourcePane);

function sourceDefault() {
  return Math.round(Math.min(640, Math.max(420, window.innerWidth * 0.38)));
}

function sourceFits() {
  return window.matchMedia('(min-width: 1000px)').matches;
}

function sourceLimit() {
  const side = parseFloat(getComputedStyle(document.body).getPropertyValue('--side')) || 0;
  const chat = document.body.classList.contains('chat-open') ? document.querySelector('aside.chat')?.getBoundingClientRect().width || 0 : 0;
  return Math.max(SOURCE_MIN, Math.floor((window.innerWidth - side - chat) * 0.52));
}

const sourceGrip = document.createElement('div');
sourceGrip.className = 'source-grip';
sourceGrip.tabIndex = 0;
sourceGrip.setAttribute('role', 'separator');
sourceGrip.setAttribute('aria-orientation', 'vertical');
sourceGrip.setAttribute('aria-label', 'Drag to resize the original');
const sourceResize = window.leoPaneResize.create({
  handle: sourceGrip,
  key: SOURCE_KEY,
  docked: 'right',
  measure: () => sourcePane.getBoundingClientRect().width,
  limits: () => [SOURCE_MIN, sourceLimit()],
  apply: (width) => document.body.style.setProperty('--source-w', `${width ?? sourceDefault()}px`),
  enabled: () => sourceFits() && !sourcePane.hidden,
});

function closeSource(control = null) {
  if (!source.note) return;
  if (control) control.focus({ preventScroll: true });
  const restore = sourcePane.contains(document.activeElement) ? source.returnTo : null;
  sourceResize.stop();
  source.seq++;
  source.note = null;
  source.name = null;
  source.returnTo = null;
  sourcePane.hidden = true;
  sourcePane.innerHTML = '';
  document.body.classList.remove('with-source');
  if (restore?.isConnected) restore.focus({ preventScroll: true });
}

async function openSource(id, name, page = null) {
  const kind = viewKind(name);
  if (!kind) return;
  const mine = ++source.seq;
  if (!source.note) source.returnTo = document.activeElement;
  source.note = id;
  source.name = name;
  document.body.classList.add('with-source');
  const url = `/api/notes/${enc(id)}/originals/${enc(name)}`;
  let files = [];
  try {
    files = (await api(`/api/notes/${enc(id)}/originals`)).filter((f) => viewKind(f.name));
  } catch (e) {
    if (e instanceof Locked) throw e;
  }
  if (source.seq !== mine || source.note !== id || source.name !== name) return;
  const picker = files.length > 1
    ? `<span class="source-files">${files.slice(0, 12).map((f) => `<button type="button" class="chip${f.name === name ? ' accent' : ''}" data-action="source-file" data-id="${esc(id)}" data-name="${esc(f.name)}">${esc(f.name)}</button>`).join('')}</span>`
    : `<b class="source-name" title="${esc(name)}">${esc(name)}</b>`;
  const body = kind === 'pdf'
    ? `<iframe class="source-frame" src="${url}?view=1${page ? `#page=${Number(page)}` : ''}" title="${esc(name)}"></iframe>`
    : kind === 'image'
      ? `<div class="source-scroll"><img class="source-img" src="${url}?view=1" alt="${esc(name)}"></div>`
      : '<div class="source-scroll prose source-text">Loading…</div>';
  sourcePane.hidden = false;
  sourcePane.innerHTML = `<header class="source-head">${picker}<span class="source-tools"><a class="btn sm plain" href="${url}" download="${esc(name)}">Download</a><button type="button" class="icon-btn" data-action="source-close" aria-label="Close the original">${ICON.close}</button></span></header>
    <div class="source-body">${body}</div>`;
  sourcePane.prepend(sourceGrip);
  sourceResize.fit();
  if (kind === 'text') {
    await fillTextFile(sourcePane.querySelector('.source-text'), url, name);
  }
}

