(function (root) {
  'use strict';

  const pad = (n) => String(n).padStart(2, '0');

  function dayKey(date) {
    return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
  }

  function fromKey(key) {
    const [y, m, d] = String(key).split('-').map(Number);
    return new Date(y, (m || 1) - 1, d || 1);
  }

  function addDays(date, n) {
    const next = new Date(date.getFullYear(), date.getMonth(), date.getDate() + n);
    return next;
  }

  function weekStart(locale) {
    try {
      const info = new Intl.Locale(locale || 'en-US');
      const week = info.getWeekInfo ? info.getWeekInfo() : info.weekInfo;
      if (week && Number.isInteger(week.firstDay)) return week.firstDay % 7;
    } catch (e) {
      return 0;
    }
    return 0;
  }

  function monthGrid(year, month, firstDay = 0) {
    const first = new Date(year, month, 1);
    const lead = (first.getDay() - firstDay + 7) % 7;
    const start = addDays(first, -lead);
    return Array.from({ length: 42 }, (_, i) => {
      const date = addDays(start, i);
      return { key: dayKey(date), day: date.getDate(), inMonth: date.getMonth() === month };
    });
  }

  function weekOf(date, firstDay = 0) {
    const lead = (date.getDay() - firstDay + 7) % 7;
    const start = addDays(date, -lead);
    return Array.from({ length: 7 }, (_, i) => dayKey(addDays(start, i)));
  }

  function eventDays(event) {
    if (event.all_day && event.date) {
      const out = [];
      const last = event.end_date ? fromKey(event.end_date) : addDays(fromKey(event.date), 1);
      for (let d = fromKey(event.date); d < last && out.length < 366; d = addDays(d, 1)) out.push(dayKey(d));
      return out;
    }
    const start = new Date(event.start);
    if (Number.isNaN(start.getTime())) return [];
    const end = new Date(event.end);
    const out = [dayKey(start)];
    if (Number.isNaN(end.getTime())) return out;
    for (let d = addDays(start, 1); d < end && out.length < 31; d = addDays(d, 1)) out.push(dayKey(d));
    return out;
  }

  const BYDAY = ['MO', 'TU', 'WE', 'TH', 'FR', 'SA', 'SU'];

  function weekdayOf(key) {
    return (fromKey(key).getDay() + 6) % 7;
  }

  function repeats(task) {
    return Array.isArray(task.repeat) && task.repeat.length > 0;
  }

  function repeatsOn(task, key) {
    return repeats(task) && task.repeat.includes(weekdayOf(key)) && (!task.date || key >= task.date);
  }

  function occurrence(task, key, slot = null) {
    const at = slot ? { date: slot.date, time: slot.time || null, end: slot.time ? slot.end || null : null } : { date: key };
    return { ...task, ...at, done: (task.done_on || []).includes(key), on: key, shifted: Boolean(slot) };
  }

  function occurrences(task, from, to) {
    const out = [];
    if (!repeats(task)) return out;
    const moved = task.moved || {};
    for (let d = fromKey(from); dayKey(d) <= to && out.length < 400; d = addDays(d, 1)) {
      const key = dayKey(d);
      if (repeatsOn(task, key) && !moved[key]) out.push(occurrence(task, key));
    }
    for (const [key, slot] of Object.entries(moved)) {
      if (slot && slot.date >= from && slot.date <= to && repeatsOn(task, key)) out.push(occurrence(task, key, slot));
    }
    return out.sort((a, b) => (a.date < b.date ? -1 : a.date > b.date ? 1 : 0));
  }

  function minutesOf(time) {
    const [h, m] = String(time || '0:0').split(':').map(Number);
    return (h || 0) * 60 + (m || 0);
  }

  function timeOf(minutes) {
    const at = Math.max(0, Math.min(24 * 60 - 1, Math.round(minutes)));
    return `${pad(Math.floor(at / 60))}:${pad(at % 60)}`;
  }

  function snap(minutes, step = 15) {
    return Math.round(minutes / step) * step;
  }

  function daysOf(view, key, firstDay = 0) {
    if (view === 'day') return [key];
    if (view === 'week') return weekOf(fromKey(key), firstDay);
    const d = fromKey(key);
    return monthGrid(d.getFullYear(), d.getMonth(), firstDay).map((c) => c.key);
  }

  function spanOn(item, key, least = 30) {
    if (item.type === 'task') {
      const start = minutesOf(item.task.time);
      const end = item.task.end ? minutesOf(item.task.end) : 0;
      return { start, end: Math.min(24 * 60, Math.max(end, start + least)) };
    }
    const from = new Date(item.event.start);
    const to = new Date(item.event.end);
    const dayStart = fromKey(key).getTime();
    const dayEnd = addDays(fromKey(key), 1).getTime();
    const start = from.getTime() <= dayStart ? 0 : from.getHours() * 60 + from.getMinutes();
    const end = Number.isNaN(to.getTime()) || to.getTime() >= dayEnd ? 24 * 60 : to.getHours() * 60 + to.getMinutes();
    return { start, end: Math.min(24 * 60, Math.max(end, start + least)) };
  }

  function dayColumns(spans) {
    const order = spans.map((s, i) => i).sort((a, b) => spans[a].start - spans[b].start || spans[b].end - spans[a].end);
    const out = spans.map(() => ({ col: 0, cols: 1 }));
    let group = [];
    let ends = [];
    let groupEnd = -1;
    const close = () => {
      for (const i of group) out[i].cols = ends.length;
      group = [];
      ends = [];
    };
    for (const i of order) {
      const { start, end } = spans[i];
      if (start >= groupEnd) close();
      let col = ends.findIndex((e) => e <= start);
      if (col < 0) col = ends.length;
      ends[col] = end;
      out[i].col = col;
      group.push(i);
      groupEnd = Math.max(groupEnd, end);
    }
    close();
    return out;
  }

  function lanes(spans) {
    const order = spans.map((s, i) => i).sort((a, b) => spans[a].a - spans[b].a || spans[b].b - spans[a].b);
    const last = [];
    const out = spans.map(() => 0);
    for (const i of order) {
      let lane = last.findIndex((b) => b < spans[i].a);
      if (lane < 0) lane = last.length;
      last[lane] = spans[i].b;
      out[i] = lane;
    }
    return out;
  }

  function shiftRepeat(repeat, days) {
    const by = ((days % 7) + 7) % 7;
    return [...new Set((repeat || []).map((d) => (d + by) % 7))].sort((a, b) => a - b);
  }

  function nextOpen(task, from, to) {
    return occurrences(task, from, to).find((o) => !o.done) || null;
  }

  function repeatLabel(repeat, locale) {
    const days = [...new Set(repeat || [])].filter((d) => d >= 0 && d < 7).sort((a, b) => a - b);
    if (!days.length) return '';
    if (days.length === 7) return 'Every day';
    if (days.join() === '0,1,2,3,4') return 'Every weekday';
    if (days.join() === '5,6') return 'Every weekend';
    const name = (d) => new Date(2024, 0, 1 + d).toLocaleDateString(locale || undefined, { weekday: 'short' });
    return `Every ${days.map(name).join(', ')}`;
  }

  function plan(tasks, events, today, range) {
    const todayKey = typeof today === 'string' ? today : dayKey(today);
    const from = (range && range.from) || dayKey(addDays(fromKey(todayKey), -42));
    const to = (range && range.to) || dayKey(addDays(fromKey(todayKey), 42));
    const byDay = new Map();
    const slot = (key) => {
      if (!byDay.has(key)) byDay.set(key, { allDay: [], timed: [] });
      return byDay.get(key);
    };
    const overdue = [];
    const undated = [];
    const linked = new Set(events.map((e) => e.task).filter(Boolean));
    for (const task of tasks) {
      if (repeats(task)) {
        for (const one of occurrences(task, from, to)) {
          const item = { type: 'task', task: one, time: one.time || null };
          if (one.time) slot(one.date).timed.push(item);
          else slot(one.date).allDay.push(item);
        }
        continue;
      }
      if (!task.date) {
        if (!task.done) undated.push(task);
        continue;
      }
      if (!task.done && task.date < todayKey) overdue.push(task);
      const item = { type: 'task', task, time: task.time || null };
      if (task.time) slot(task.date).timed.push(item);
      else slot(task.date).allDay.push(item);
    }
    for (const event of events) {
      if (event.task && tasks.some((t) => t.id === event.task)) continue;
      for (const key of eventDays(event)) {
        const item = { type: 'event', event, time: event.all_day ? null : event.start };
        if (event.all_day) slot(key).allDay.push(item);
        else slot(key).timed.push(item);
      }
    }
    for (const day of byDay.values()) {
      day.timed.sort((a, b) => sortTime(a) - sortTime(b));
    }
    return { byDay, overdue, undated, linked };
  }

  function sortTime(item) {
    if (item.type === 'event') return new Date(item.event.start).getTime();
    const [h, m] = String(item.task.time || '00:00').split(':').map(Number);
    return fromKey(item.task.date).getTime() + ((h || 0) * 60 + (m || 0)) * 60000;
  }

  function nextWeekday(date, weekday) {
    const ahead = (weekday - date.getDay() + 7) % 7 || 7;
    return addDays(date, ahead);
  }

  function quickDates(today) {
    return [
      { label: 'Today', key: dayKey(today) },
      { label: 'Tomorrow', key: dayKey(addDays(today, 1)) },
      { label: today.getDay() === 5 ? 'Next Fri' : 'Fri', key: dayKey(nextWeekday(today, 5)) },
      { label: 'Next week', key: dayKey(nextWeekday(today, 1)) },
    ];
  }

  function stamp(key, time) {
    const [y, m, d] = String(key).split('-');
    if (!time) return `${y}${m}${d}`;
    const [h, min] = String(time).split(':');
    return `${y}${m}${d}T${pad(Number(h))}${pad(Number(min))}00`;
  }

  function shifted(key, time, minutes) {
    const [h, m] = String(time).split(':').map(Number);
    const at = fromKey(key);
    at.setHours(h || 0, (m || 0) + minutes, 0, 0);
    return { key: dayKey(at), time: `${pad(at.getHours())}:${pad(at.getMinutes())}` };
  }

  function firstDay(task) {
    if (!repeats(task)) return task.date || null;
    const start = task.date || dayKey(new Date());
    const found = occurrences({ ...task, date: start }, start, dayKey(addDays(fromKey(start), 7)));
    return found.length ? found[0].date : start;
  }

  function rrule(repeat) {
    const days = [...new Set(repeat || [])].filter((d) => d >= 0 && d < 7).sort((a, b) => a - b);
    return days.length ? `RRULE:FREQ=WEEKLY;BYDAY=${days.map((d) => BYDAY[d]).join(',')}` : '';
  }

  function googleLink(task, zone) {
    if (task && repeats(task)) task = { ...task, date: firstDay(task) };
    if (!task || !task.date) return null;
    let dates;
    let title = task.title;
    if (!task.time) {
      dates = `${stamp(task.date)}/${stamp(dayKey(addDays(fromKey(task.date), 1)))}`;
    } else if (task.end) {
      dates = `${stamp(task.date, task.time)}/${stamp(task.date, task.end)}`;
    } else {
      const start = shifted(task.date, task.time, -30);
      dates = `${stamp(start.key, start.time)}/${stamp(task.date, task.time)}`;
      if (task.kind !== 'meeting') title = `Due: ${task.title}`;
    }
    const from = task.note && task.note.title ? ` · ${task.note.title}` : '';
    const params = new URLSearchParams({ action: 'TEMPLATE', text: title, dates, details: `${task.description ? `${task.description}\n\n` : ''}From leo${from} · leo-task:${String(task.id).slice(0, 12)}` });
    if (zone) params.set('ctz', zone);
    if (repeats(task)) params.set('recur', rrule(task.repeat));
    return `https://calendar.google.com/calendar/render?${params.toString()}`;
  }

  function googleDay(key) {
    const [y, m, d] = String(key).split('-').map(Number);
    return `https://calendar.google.com/calendar/r/day/${y}/${m}/${d}`;
  }

  function timeLabel(time, locale) {
    if (!time) return '';
    const [h, m] = String(time).split(':').map(Number);
    const at = new Date(2000, 0, 1, h || 0, m || 0);
    return at.toLocaleTimeString(locale || undefined, { hour: 'numeric', minute: '2-digit' });
  }

  const TEXT_ESCAPES = { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' };
  const escapeText = (text) => String(text).replace(/[&<>"']/g, (c) => TEXT_ESCAPES[c]);
  const NAMED = { amp: '&', lt: '<', gt: '>', quot: '"', apos: "'", nbsp: ' ', '#39': "'" };
  const decode = (text) => String(text).replace(/&(#x[0-9a-f]+|#\d+|[a-z]+|#39);/gi, (all, name) => {
    const lower = name.toLowerCase();
    if (NAMED[lower] !== undefined) return NAMED[lower];
    const code = lower.startsWith('#x') ? parseInt(lower.slice(2), 16) : lower.startsWith('#') ? parseInt(lower.slice(1), 10) : NaN;
    return Number.isFinite(code) && code > 0 && code < 0x110000 ? String.fromCodePoint(code) : all;
  });
  const linkify = (escaped) => escaped.replace(/\bhttps?:\/\/[^\s<>"']+[^\s<>"'.,;:!?)\]]/g, (url) => `<a href="${url}" target="_blank" rel="noopener noreferrer">${url}</a>`);
  const KEEP = { strong: 'strong', b: 'strong', em: 'em', i: 'em', u: 'u', ul: 'ul', ol: 'ol', li: 'li', p: 'p', h1: 'strong', h2: 'strong', h3: 'strong', h4: 'strong' };
  const BREAKS = new Set(['br', 'div', 'tr', 'hr']);

  function richText(input) {
    const raw = String(input || '').replace(/<(script|style|head|title)\b[^>]*>[\s\S]*?(<\/\1\s*>|$)/gi, '');
    if (!/<\/?[a-z][a-z0-9]*\b[^>]*>/i.test(raw)) return linkify(escapeText(decode(raw))).replace(/\r?\n/g, '<br>');
    const open = [];
    let out = '';
    for (const part of raw.split(/(<[^<>]*>)/)) {
      if (!part) continue;
      const tag = /^<\s*(\/?)\s*([a-z][a-z0-9]*)\b([^>]*)>$/i.exec(part);
      if (!tag) {
        const text = escapeText(decode(part)).replace(/\r?\n/g, '<br>');
        out += open.includes('a') ? text : linkify(text);
        continue;
      }
      const closing = tag[1] === '/';
      const name = tag[2].toLowerCase();
      if (BREAKS.has(name)) {
        if (name === 'br' || closing) out += '<br>';
        continue;
      }
      if (name === 'a') {
        if (closing) {
          if (open.includes('a')) {
            while (open.length) { const last = open.pop(); out += `</${last}>`; if (last === 'a') break; }
          }
          continue;
        }
        const href = /href\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))/i.exec(tag[3]);
        const url = href ? decode(href[1] ?? href[2] ?? href[3] ?? '').trim() : '';
        if (/^(https?:\/\/|mailto:)/i.test(url) && !open.includes('a')) {
          out += `<a href="${escapeText(url)}" target="_blank" rel="noopener noreferrer">`;
          open.push('a');
        }
        continue;
      }
      const kept = KEEP[name];
      if (!kept) continue;
      if (!closing) {
        out += `<${kept}>`;
        open.push(kept);
      } else if (open.includes(kept)) {
        while (open.length) { const last = open.pop(); out += `</${last}>`; if (last === kept) break; }
      }
    }
    while (open.length) out += `</${open.pop()}>`;
    return out.replace(/^(?:<br>)+|(?:<br>)+$/g, '').replace(/(?:<br>){3,}/g, '<br><br>');
  }

  function eventDetails(context) {
    const lines = String(context || '').split('\n');
    const details = { place: '', organiser: '', invited: [], description: '' };
    let at = 1;
    for (; at < lines.length; at++) {
      const line = lines[at];
      if (line.startsWith('Place: ')) details.place = line.slice(7).trim();
      else if (line.startsWith('Organised by: ')) details.organiser = line.slice(14).trim();
      else if (line.startsWith('Invited: ')) details.invited = line.slice(9).split(',').map((p) => p.trim()).filter(Boolean);
      else if (line === 'Description:') { details.description = lines.slice(at + 1).join('\n').trim(); break; }
      else { details.description = lines.slice(at).join('\n').trim(); break; }
    }
    if (/@group\.calendar\.google\.com$|@resource\.calendar\.google\.com$|@import\.calendar\.google\.com$/i.test(details.organiser)) details.organiser = 'A shared calendar';
    details.invited = details.invited.filter((p) => !/@(group|resource|import)\.calendar\.google\.com$/i.test(p));
    return details;
  }

  const DAYS = ['sunday', 'monday', 'tuesday', 'wednesday', 'thursday', 'friday', 'saturday'];
  const ASK = /^(what|why|how|when|who|whom|where|which|whose|can|could|does|do|did|is|are|was|were|should|would|will|explain|summari[sz]e|tell me|help me)\b/i;

  const NUDGE = /\b(?:(?:can|could|would) you\s+|please\s+)?(?:remind me(?:\s+to)?|don'?t forget to|do not forget to)\b\s*[:\-]?/i;

  function quickParse(input, now = new Date()) {
    let text = String(input || '').trim();
    let kind = '';
    const strip = (re) => {
      const before = text;
      text = text.replace(re, ' ').replace(/\s{2,}/g, ' ').trim();
      return before !== text;
    };
    if (/^note\s*[:\-]\s*/i.test(text)) { kind = 'note'; text = text.replace(/^note\s*[:\-]\s*/i, ''); }
    else if (/^(remind me( to)?|todo|to do|task)\s*[:\-]?\s+/i.test(text)) { kind = 'task'; text = text.replace(/^(remind me( to)?|todo|to do|task)\s*[:\-]?\s+/i, ''); }
    else if (NUDGE.test(text)) { kind = 'task'; strip(NUDGE); }
    const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
    let date = null;
    let time = null;
    const clock = /(?:\b(?:at|by|@)\s*)?\b(\d{1,2})(?::(\d{2}))?\s*(am|pm|a\.m\.|p\.m\.)(?=\s|$|[.,!?])|(?:\b(?:at|by|@)\s*)(\d{1,2}):(\d{2})\b|\b(\d{1,2}):(\d{2})\b|\b(?:at\s+)?(noon|midnight)\b/i;
    const t = clock.exec(text);
    if (t) {
      let h; let m = 0;
      if (t[1]) { h = Number(t[1]) % 12; m = Number(t[2] || 0); if (/^p/i.test(t[3])) h += 12; }
      else if (t[4]) { h = Number(t[4]); m = Number(t[5]); }
      else if (t[6]) { h = Number(t[6]); m = Number(t[7]); }
      else { h = /noon/i.test(t[8]) ? 12 : 0; }
      if (h < 24 && m < 60) {
        time = `${pad(h)}:${pad(m)}`;
        text = (text.slice(0, t.index) + ' ' + text.slice(t.index + t[0].length)).replace(/\s{2,}/g, ' ').trim();
      }
    }
    const named = /\b(jan(?:uary)?|feb(?:ruary)?|mar(?:ch)?|apr(?:il)?|may|jun(?:e)?|jul(?:y)?|aug(?:ust)?|sep(?:tember)?|oct(?:ober)?|nov(?:ember)?|dec(?:ember)?)\s+(\d{1,2})(?:(?:,\s*|\s+)(\d{4}))?\b/i.exec(text);
    const iso = /\b(\d{4})-(\d{2})-(\d{2})(?:T(\d{2}):(\d{2}))?\b(?![:+-])/i.exec(text);
    const explicit = iso || named;
    if (explicit) {
      const month = iso ? Number(iso[2]) - 1 : ['jan', 'feb', 'mar', 'apr', 'may', 'jun', 'jul', 'aug', 'sep', 'oct', 'nov', 'dec'].indexOf(named[1].slice(0, 3).toLowerCase());
      const day = Number(iso ? iso[3] : named[2]);
      const year = Number(iso ? iso[1] : named[3] || today.getFullYear());
      let candidate = new Date(year, month, day);
      if (candidate.getFullYear() === year && candidate.getMonth() === month && candidate.getDate() === day && (!iso?.[4] || (Number(iso[4]) < 24 && Number(iso[5]) < 60))) {
        if (!iso && !named[3] && candidate < today) candidate = new Date(year + 1, month, day);
        if (candidate.getMonth() === month && candidate.getDate() === day) {
          date = candidate;
          if (iso?.[4]) time = `${iso[4]}:${iso[5]}`;
          text = (text.slice(0, explicit.index) + ' ' + text.slice(explicit.index + explicit[0].length)).replace(/\s{2,}/g, ' ').trim();
        }
      }
    }
    const inDays = /\bin\s+(\d{1,3})\s+days?\b/i.exec(text);
    if (!explicit) {
      if (/\btonight\b/i.test(text)) { date = today; if (!time) time = '20:00'; strip(/\btonight\b/i); }
      else if (/\btoday\b/i.test(text)) { date = today; strip(/\btoday\b/i); }
      else if (/\b(tomorrow|tmrw|tmr)\b/i.test(text)) { date = addDays(today, 1); strip(/\b(tomorrow|tmrw|tmr)\b/i); }
      else if (/\bnext week\b/i.test(text)) { date = nextWeekday(today, 1); strip(/\bnext week\b/i); }
      else if (inDays) { date = addDays(today, Number(inDays[1])); strip(/\bin\s+\d{1,3}\s+days?\b/i); }
      else {
        const day = [...text.matchAll(/\b(?:(on|this|next|for|by|until|till)\s+)?(sun|mon|tue|tues|wed|thu|thur|thurs|fri|sat)(?:day|nesday|sday|urday|rsday)?\b/gi)]
          .find((d) => d[1] || d[0].length > 3 || !/^(sun|sat)$/i.test(d[2]) || /^\s*([,.;!?]|$)/.test(text.slice(d.index + d[0].length)));
        if (day) {
          const index = DAYS.findIndex((d) => d.startsWith(day[2].toLowerCase().slice(0, 3)));
          if (index >= 0) {
            date = nextWeekday(today, index);
            if (index === today.getDay() && !/next/i.test(day[1] || '')) date = today;
            if (/next/i.test(day[1] || '') && (date - today) / 86400000 < 7 && index !== today.getDay()) date = addDays(date, 7);
            text = (text.slice(0, day.index) + ' ' + text.slice(day.index + day[0].length)).replace(/\s{2,}/g, ' ').trim();
          }
        }
      }
    }
    if (time && !date && !explicit) date = today;
    text = text.replace(/^[\s,.;:!?-]+/, '').replace(/\s+(on|at|by|for|due)$/i, '').replace(/^(on|at|by|for|to)\s+/i, '').replace(/[\s,.;:!?]+$/, '').trim();
    if (!kind) kind = /\?$/.test(String(input).trim()) || ASK.test(String(input).trim()) ? 'ask' : date ? 'task' : 'ask';
    return { kind, title: kind === 'ask' ? String(input).trim() : text, date: date ? dayKey(date) : null, time };
  }

  const TODO = /^(buy|get|grab|pick up|pick|drop off|bring|order|return|pay|book|call|phone|ring|email|e-mail|text|message|reply to|reply|respond to|follow up|send|ask|tell|remind|invite|meet|visit|finish|submit|file|sign|apply|register|renew|cancel|schedule|reschedule|plan|prepare|prep|pack|print|clean|wash|tidy|cook|water|feed|walk|fix|repair|install|update|back up|backup|charge|study|practice|practise|review|revise|read|write|draft|edit|work on|look into|research|check|move|sell|donate|shop for|go to|take|make|post|upload|download|watch|listen to|learn)\b/i;

  const STUDY = /\b(compare|explain|summari[sz]e|table|why|how)\b/i;

  function quickRank(input, { notes = [], places = [], folders = [], chats = [] } = {}, now = new Date()) {
    const text = String(input || '').trim();
    if (!text) return [];
    const parsed = quickParse(text, now);
    const lower = text.toLowerCase();
    const words = text.split(/\s+/).length;
    const asked = /\?$/.test(text) || ASK.test(text);
    const reminder = parsed.kind === 'task' ? parsed : quickParse(`remind me to ${text}`, now);
    const titled = (n) => String(n.title || '').toLowerCase();
    const named = notes.filter((n) => titled(n) === lower);
    const started = notes.filter((n) => titled(n) !== lower && titled(n).startsWith(lower));
    const opens = [...named, ...started, ...notes.filter((n) => !named.includes(n) && !started.includes(n))].slice(0, 3).map((n) => ({ kind: 'open', id: n.id, title: n.title }));
    const leaf = (path) => String(path).split('/').pop().toLowerCase();
    const shelf = lower.length >= 2 ? folders.filter((f) => leaf(f).startsWith(lower) || leaf(f).split(/[\s_-]+/).some((w) => w.startsWith(lower))) : [];
    const shelves = [...shelf.filter((f) => leaf(f) === lower), ...shelf.filter((f) => leaf(f) !== lower)].slice(0, 2).map((path) => ({ kind: 'folder', path }));
    const talks = chats.slice(0, 2).map((c) => ({ kind: 'chat', id: c.id, title: c.title }));
    const goes = lower.length >= 2 ? places.filter((p) => p.name.toLowerCase().split(/\s+/).some((w) => w.startsWith(lower)) || p.name.toLowerCase().startsWith(lower)).slice(0, 2).map((p) => ({ kind: 'place', name: p.name, hash: p.hash })) : [];
    const ask = { kind: 'ask', text };
    const task = { kind: 'task', title: reminder.title || text, date: reminder.date, time: reminder.time };
    const note = { kind: 'note', title: parsed.kind === 'note' ? parsed.title : text };
    const search = { kind: 'search', text };
    let top = ask;
    if (parsed.kind === 'note') top = note;
    else if (parsed.kind === 'task') top = task;
    else if (asked) top = ask;
    else if (named.length) top = opens[0];
    else if (shelves.length && leaf(shelves[0].path) === lower) top = shelves[0];
    else if (goes.length && words <= 2 && lower.length >= 3 && (!started.length || goes[0].name.toLowerCase().startsWith(lower))) top = goes[0];
    else if (started.length && words <= 4) top = opens[0];
    else if (TODO.test(text) && words >= 2 && words <= 6 && !STUDY.test(text)) top = task;
    const rows = [top, ask, ...opens, search, ...shelves, ...talks, ...goes, task, note].filter((row, i, all) => all.indexOf(row) === i);
    const found = rows.filter((row) => row !== task && row !== note).slice(0, 6);
    return rows.filter((row) => found.includes(row) || row === task || row === note);
  }


  const COUNTS = ['no', 'one', 'two', 'three', 'four', 'five', 'six', 'seven', 'eight', 'nine'];
  const counted = (n) => (n < COUNTS.length ? COUNTS[n] : String(n));
  const upper = (text) => text.charAt(0).toUpperCase() + text.slice(1);

  function homeDay(tasks, now, keep = new Set()) {
    const today = dayKey(now);
    const earlier = [];
    const day = [];
    for (const task of tasks) {
      if (repeats(task)) day.push(...occurrences(task, today, today));
      else if (task.date === today) day.push(task);
      else if (task.date && task.date < today && (!task.done || keep.has(task.id))) earlier.push(task);
    }
    const at = (t) => (t.time ? minutesOf(t.time) : 24 * 60);
    earlier.sort((a, b) => (a.date < b.date ? -1 : a.date > b.date ? 1 : at(a) - at(b)));
    day.sort((a, b) => at(a) - at(b));
    return { earlier, today: day };
  }

  function homeItems(tasks, events, now) {
    const from = dayKey(now);
    const to = dayKey(addDays(now, 7));
    const listed = new Set(tasks.map((t) => t.id));
    const out = [];
    for (const task of tasks) {
      for (const t of repeats(task) ? occurrences(task, from, to) : [task]) {
        if (t.done || !t.date || t.date < from || t.date > to) continue;
        const at = t.time ? fromKey(t.date) : null;
        if (at) at.setMinutes(minutesOf(t.time));
        out.push({ title: t.title, day: t.date, at });
      }
    }
    for (const event of events) {
      if (event.task && listed.has(event.task)) continue;
      if (event.all_day) {
        for (const key of eventDays(event)) if (key >= from && key <= to) out.push({ title: event.title, day: key, at: null, allDay: true });
        continue;
      }
      const start = new Date(event.start);
      const end = new Date(event.end);
      if (Number.isNaN(start.getTime()) || dayKey(start) > to) continue;
      out.push({ title: event.title, day: dayKey(start), at: start, end: Number.isNaN(end.getTime()) ? null : end, event: true });
    }
    return out;
  }

  function dateline({ tasks = [], events = [], now = new Date(), notes = null, locale } = {}) {
    const time = (d) => timeLabel(`${d.getHours()}:${d.getMinutes()}`, locale);
    const title = (text) => {
      const chars = Array.from(String(text || '').trim() || 'Untitled');
      return { title: chars.length > 40 ? `${chars.slice(0, 40).join('').trimEnd()}…` : chars.join('') };
    };
    const starts = (item) => (item.at ? [title(item.title), ` at ${time(item.at)}.`] : [title(item.title), item.allDay ? ' all day.' : '.']);
    const first = (list) => list.find((item) => item.at) || list[0];
    const todayKey = dayKey(now);
    const tomorrow = dayKey(addDays(now, 1));
    const { earlier, today } = homeDay(tasks, now);
    const items = homeItems(tasks, events, now);
    const later = items.filter((item) => item.at && item.at > now && item.day === todayKey).sort((a, b) => a.at - b.at);
    const open = today.filter((t) => !t.done);
    const andEarlier = earlier.length ? ` and ${counted(earlier.length)} from earlier` : '';
    const still = earlier.length === 1 ? 'One from earlier is still open.' : `${upper(counted(earlier.length))} from earlier are still open.`;
    if (notes === 0 && !tasks.length) return ['Nothing here yet. Type in the bar to jot a note or a task.'];
    const on = items.filter((item) => item.event && item.at <= now && item.end && item.end > now).sort((a, b) => a.at - b.at)[0];
    if (on) return [title(on.title), ` until ${time(on.end)}.`, ...(later.length ? [' Then ', ...starts(later[0])] : [])];
    if (now.getHours() < 4 && later.length) return ['Today starts with ', ...starts(later[0])];
    if (open.length >= 6) return [`${upper(counted(open.length))} today${andEarlier}.`, ...(later.length ? [' First up is ', ...starts(later[0])] : [])];
    if (open.length === 1) return [`One left today${andEarlier}: `, title(open[0].title), open[0].time ? ` at ${timeLabel(open[0].time, locale)}.` : '.'];
    if (open.length) {
      const head = `${upper(counted(open.length))} left today${andEarlier}`;
      if (later.length) return [`${head}. `, title(later[0].title), ` is at ${time(later[0].at)}.`];
      return [open.some((t) => t.time) ? `${head}.` : `${head}, none at a set time.`];
    }
    if (today.length) {
      const done = today.length === 1 ? 'Today’s one task is done.' : `All ${counted(today.length)} done.`;
      const next = first(items.filter((item) => item.day === tomorrow));
      if (next) return [`${done} Tomorrow starts with `, ...starts(next)];
      return [earlier.length ? `${done} ${still}` : done];
    }
    const allDay = items.find((item) => item.allDay && item.day === todayKey);
    if (later.length || allDay) {
      const lead = later.length ? starts(later[0]) : starts(allDay);
      if (later.length > 1) return [...lead, ' Then ', ...starts(later[1])];
      return earlier.length ? [...lead, ` ${still}`] : lead;
    }
    if (earlier.length) return [`Nothing new today. ${still}`];
    const ahead = items.filter((item) => item.day > todayKey);
    const soonest = ahead.reduce((min, item) => (item.day < min ? item.day : min), '9999');
    const next = first(ahead.filter((item) => item.day === soonest));
    if (!next) return ['Nothing scheduled today.'];
    const weekday = fromKey(next.day).toLocaleDateString(locale || undefined, { weekday: 'long' });
    const when = `${next.day === tomorrow ? 'tomorrow' : weekday}${next.at ? ` at ${time(next.at)}` : ''}.`;
    const sunday = dayKey(addDays(now, now.getDay() === 6 ? 1 : 0));
    if ((now.getDay() === 0 || now.getDay() === 6) && next.day > sunday) return ['Nothing on until ', title(next.title), `, ${weekday}${next.at ? ` at ${time(next.at)}` : ''}.`];
    if (next.day === tomorrow && now.getHours() >= 17) return ['Nothing scheduled today. Tomorrow starts with ', ...starts(next)];
    return ['Nothing scheduled today. Next is ', title(next.title), `, ${when}`];
  }

  const api = { homeDay, dateline, minutesOf, timeOf, snap, daysOf, spanOn, dayColumns, lanes, shiftRepeat, quickParse, quickRank, richText, eventDetails, dayKey, fromKey, addDays, weekStart, monthGrid, weekOf, eventDays, plan, quickDates, weekdayOf, repeats, repeatsOn, occurrences, nextOpen, repeatLabel, rrule, googleLink, googleDay, timeLabel, nextWeekday };
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
  else root.leoSchedule = api;
})(typeof window !== 'undefined' ? window : globalThis);
