(function (root) {
  'use strict';

  const KEY = 'leo-chat-v1';
  const noAi = (text) => typeof text === 'string' && text.startsWith("leo can't write yet: ");
  const MOST_KEPT = 40;
  const MOST_REFS = 8;
  const MOST_SAVED = 400;
  const COMPACT_IDLE_MS = 10 * 60 * 1000;
  const COMPACT_LEAST_TURNS = 8;
  const ACCESS = [
    { id: 'ask', label: 'Ask before changing notes', short: 'Ask first', hint: 'Felix suggests changes and new notes; you apply each one.' },
    { id: 'auto', label: 'Change notes automatically', short: 'Auto', hint: 'Felix makes changes and new notes himself; each one can be undone.' },
    { id: 'read', label: 'Read only', short: 'Read only', hint: 'Felix reads your notes but never changes or makes any.' },
  ];
  const ACCESS_KEY = 'leo-felix-access';
  const accessOf = (id) => (ACCESS.some((a) => a.id === id) ? id : 'ask');
  const nextAccess = (id) => ACCESS[(ACCESS.findIndex((a) => a.id === accessOf(id)) + 1) % ACCESS.length].id;
  const ACCESS_GLYPHS = {
    ask: '<path d="M12 3l7 3v5c0 4.5-3 8-7 10-4-2-7-5.5-7-10V6z"/><path d="M9.5 12l2 2 3.5-4"/>',
    auto: '<path d="M5 12h9M10 7l5 5-5 5"/><path d="M15 7l5 5-5 5"/>',
    read: '<path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12z"/><circle cx="12" cy="12" r="3"/>',
  };

  const NEW_CHAT = '<button class="icon-btn" data-chat="new" aria-label="New chat" title="New chat"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><path d="M12 5v14M5 12h14"/></svg></button>';
  const STARTERS = [
    { text: 'Teach me this note step by step', needs: 'note', ask: 'Which note should I teach you?' },
    { text: 'Quiz me on this note', needs: 'note', ask: 'Which note should the quiz cover?' },
    { text: 'Make me a 3-day review plan', needs: 'classes', ask: 'Which classes should the plan cover?' },
  ];
  const SURE = { sure: 'Sure', think: 'Think so', guess: 'Guessing' };
  const sureOf = (percent) => (percent >= 75 ? 'sure' : percent >= 40 ? 'think' : 'guess');
  const sureText = (quiz) => (Number.isFinite(quiz.surePct) ? `${quiz.surePct}% sure` : SURE[quiz.sure] || '');
  const MOST_THINKING = 2000;

  function containChatPickWheel(list, event) {
    if (event.ctrlKey || !event.deltaY) return;
    const max = list.scrollHeight - list.clientHeight;
    if (max <= 0 || (event.deltaY < 0 && list.scrollTop <= 0) || (event.deltaY > 0 && list.scrollTop >= max)) event.preventDefault();
  }

  function revealChatPick(list) {
    const selected = list.querySelector('.chat-pick-row.on');
    if (!selected) return;
    const top = selected.offsetTop - list.offsetTop;
    const bottom = top + selected.offsetHeight;
    if (top < list.scrollTop) list.scrollTop = top;
    else if (bottom > list.scrollTop + list.clientHeight) list.scrollTop = bottom - list.clientHeight;
  }

  function felix(width, extra = '') {
    return `<svg class="felix ${extra}" viewBox="-2 -10 50 42" width="${width}" height="${Math.round((width * 42) / 50)}" shape-rendering="crispEdges" aria-hidden="true">
      <g class="felix-spark"><rect x="-1" y="-6" width="2" height="2"/><rect x="42" y="-4" width="2" height="2"/><rect x="21" y="-8" width="2" height="2"/></g>
      <g class="felix-heart"><rect x="17" y="-9" width="2" height="1"/><rect x="21" y="-9" width="2" height="1"/><rect x="16" y="-8" width="8" height="2"/><rect x="17" y="-6" width="6" height="1"/><rect x="18" y="-5" width="4" height="1"/><rect x="19" y="-4" width="2" height="1"/></g>
      <g class="felix-thought"><rect x="37" y="-1" width="2" height="2"/><rect x="40" y="-4" width="2" height="2"/><rect x="43" y="-7" width="3" height="3"/></g>
      <g class="felix-z"><text x="37" y="-2" font-size="10" font-weight="800" font-family="system-ui, sans-serif">z</text></g>
      <g class="felix-rawr"><text x="38" y="2" font-size="7" font-weight="900" font-family="system-ui, sans-serif">RAWR!</text></g>
      <g class="felix-hehe"><text x="38" y="2" font-size="6" font-weight="900" font-family="system-ui, sans-serif">heh heh</text></g>
      <g class="felix-hi"><text x="38" y="2" font-size="7" font-weight="900" font-family="system-ui, sans-serif">hi!!</text></g>
      <g class="felix-slash"><rect x="41" y="8" width="1" height="2"/><rect x="42" y="10" width="1" height="2"/><rect x="43" y="12" width="1" height="2"/><rect x="43" y="6" width="1" height="2"/><rect x="44" y="8" width="1" height="2"/><rect x="45" y="10" width="1" height="2"/><rect x="45" y="4" width="1" height="2"/><rect x="46" y="6" width="1" height="2"/><rect x="47" y="8" width="1" height="2"/></g>
      <g class="felix-body">
        <g class="felix-arm felix-left"><rect class="felix-arm-skin" x="0" y="18" width="5" height="6"/><g class="felix-claws"><rect x="0" y="24" width="1" height="2"/><rect x="2" y="24" width="1" height="2"/><rect x="4" y="24" width="1" height="2"/></g></g>
        <g class="felix-arm felix-right"><rect class="felix-arm-skin" x="39" y="18" width="5" height="6"/><g class="felix-claws"><rect x="39" y="24" width="1" height="2"/><rect x="41" y="24" width="1" height="2"/><rect x="43" y="24" width="1" height="2"/></g></g>
        <rect class="felix-skin" x="5" y="2" width="34" height="28"/>
        <g class="felix-cheeks"><rect x="8" y="21" width="4" height="2"/><rect x="24" y="21" width="4" height="2"/></g>
        <g class="felix-gaze"><g class="felix-eyes" shape-rendering="geometricPrecision"><rect x="11" y="16" width="4" height="4"/><rect x="21" y="16" width="4" height="4"/></g></g>
        <g class="felix-sweat"><rect x="34" y="5" width="2" height="2"/><rect x="33" y="7" width="4" height="3"/></g>
        <g class="felix-tool felix-tool-search"><rect class="glass" x="20" y="14" width="8" height="8"/><rect class="metal" x="20" y="12" width="8" height="2"/><rect class="metal" x="20" y="22" width="8" height="2"/><rect class="metal" x="18" y="14" width="2" height="8"/><rect class="metal" x="28" y="14" width="2" height="8"/><rect class="wood" x="30" y="23" width="2" height="2"/><rect class="wood" x="32" y="25" width="2" height="2"/><rect class="wood" x="34" y="27" width="3" height="3"/></g>
        <g class="felix-tool felix-tool-open"><rect class="paper-old" x="10" y="22" width="24" height="8"/><rect class="ink-old" x="13" y="24" width="14" height="1"/><rect class="ink-old" x="13" y="27" width="10" height="1"/><rect class="roll" x="7" y="21" width="4" height="10"/><rect class="roll" x="33" y="21" width="4" height="10"/></g>
        <g class="felix-tool felix-tool-map"><rect class="paper-old" x="30" y="5" width="15" height="11"/><rect class="fold" x="35" y="5" width="1" height="11"/><rect class="fold" x="40" y="5" width="1" height="11"/><rect class="pin" x="32" y="8" width="2" height="2"/><rect class="pin" x="42" y="7" width="2" height="2"/><rect class="pin" x="37" y="12" width="2" height="2"/></g>
        <g class="felix-tool felix-tool-edit"><rect class="paper" x="8" y="21" width="16" height="10"/><rect class="ink" x="10" y="24" width="11" height="1"/><rect class="ink" x="10" y="27" width="8" height="1"/><g class="pen"><rect class="cap" x="29" y="18" width="2" height="2"/><rect class="barrel" x="27" y="20" width="2" height="2"/><rect class="barrel" x="25" y="22" width="2" height="2"/><rect class="tip" x="23" y="24" width="2" height="2"/></g></g>
        <g class="felix-monster"><g class="felix-horns"><rect x="9" y="-1" width="4" height="3"/><rect x="10" y="-3" width="2" height="2"/><rect x="31" y="-1" width="4" height="3"/><rect x="32" y="-3" width="2" height="2"/></g><g class="felix-spikes"><rect x="16" y="0" width="2" height="2"/><rect x="20" y="-1" width="3" height="3"/><rect x="25" y="0" width="2" height="2"/></g><g class="felix-brows"><rect x="9" y="12" width="2" height="1"/><rect x="11" y="13" width="2" height="1"/><rect x="13" y="14" width="2" height="1"/><rect x="25" y="12" width="2" height="1"/><rect x="23" y="13" width="2" height="1"/><rect x="21" y="14" width="2" height="1"/></g><rect class="felix-maw" x="13" y="23" width="12" height="4"/><g class="felix-fangs"><rect x="14" y="23" width="2" height="2"/><rect x="22" y="23" width="2" height="2"/></g></g>
        <g class="felix-flames"><rect class="hot" x="8" y="-2" width="3" height="4"/><rect class="warm" x="9" y="-5" width="2" height="3"/><rect class="hot" x="15" y="-4" width="3" height="6"/><rect class="warm" x="16" y="-8" width="2" height="4"/><rect class="hot" x="21" y="-3" width="3" height="5"/><rect class="warm" x="22" y="-6" width="2" height="3"/><rect class="hot" x="27" y="-5" width="3" height="7"/><rect class="warm" x="28" y="-9" width="2" height="4"/><rect class="hot" x="33" y="-2" width="3" height="4"/><rect class="warm" x="34" y="-4" width="2" height="2"/></g>
        <g class="felix-tool felix-tool-create"><rect class="wood" x="42" y="5" width="2" height="15"/><rect class="metal" x="38" y="1" width="10" height="5"/><rect class="shine" x="39" y="2" width="2" height="1"/></g>
      </g>
    </svg>`;
  }

  const TAPS = ['boop', 'hop', 'spin', 'giggle', 'tickle'];
  const DEMON_TAPS = ['rawr', 'cackle', 'blaze', 'glare', 'swipe'];
  const MONSTER_ONE_IN = 10;
  const TICKLED_TOO_LONG = 8;
  const STREAK_GAP_MS = 1500;
  const TRANSFORM_MS = 2800;
  const demonMode = () => typeof document !== 'undefined' && document.documentElement.dataset.felix === 'demon';
  const POSES = { search_notes: 'tool-search', open_note: 'tool-open', connected_notes: 'tool-map', edit_note: 'tool-edit', create_note: 'tool-create', read_document: 'tool-open', read_transcript: 'tool-open', look_at_picture: 'tool-search', calculate: 'tool-edit' };
  const poseOf = (tool) => POSES[tool] || null;

  function splitLines(buffer) {
    const parts = buffer.split('\n');
    const rest = parts.pop();
    const events = [];
    for (const line of parts) {
      if (!line.trim()) continue;
      try {
        events.push(JSON.parse(line));
      } catch (e) {
        events.push({ error: 'leo sent something it could not read.' });
      }
    }
    return { events, rest };
  }

  const VERDICT_WITHIN = 240;
  const QUIZ_MARK_MS = 160;
  const BLOT = '<svg class="msg-step-busy" role="img" aria-label="working"><use href="#leo-mark"/></svg>';
  const DELETE_MARK = '<svg class="proposal-loop" width="11" height="12" viewBox="0 0 11 12" aria-hidden="true"><path d="M0.5 8.1 Q5.2 7.6 7.6 4.6 Q9.4 1.9 7.2 1.3 Q4.9 1.1 5.6 4.6 Q6.4 8.4 10.2 10.6"/></svg>';
  const STUDY_MARKS = {
    rings: ['M9 2.3 Q14.9 2.6 15.4 8.7 Q15.2 15.2 8.8 15.5 Q2.6 15.2 2.4 9.1 Q2.7 3.2 9.6 2.4', 'M9.2 2.4 Q15.3 2.9 15.3 9.2 Q14.8 15.4 8.7 15.3 Q2.5 14.9 2.6 8.8 Q2.9 2.6 9.8 2.6', 'M8.8 2.5 Q15.1 2.4 15.5 9 Q15.4 15.1 9.2 15.6 Q2.4 15.4 2.3 8.9 Q2.4 2.9 9.4 2.3'],
    ok: 'M5.6 9.4 Q7 10.4 8.1 12.3 Q10.1 8.2 14.6 3.6',
    no: 'M6.2 6 Q9.1 9.1 12 12.2 M11.9 6.2 Q9 9 6 12',
  };
  const CARET_MARK = '<svg class="proposal-caret" width="10" height="7" viewBox="0 0 10 7" aria-hidden="true"><path d="M1.2 6.2 Q3.4 3.6 5.1 1.1 Q6.7 3.7 8.9 6"/></svg>';

  function grade(text) {
    const value = String(text || '');
    const match = /\[\[(correct|incorrect)\]\]\s*/i.exec(value);
    if (match && match.index <= VERDICT_WITHIN) return { verdict: match[1].toLowerCase(), text: value.slice(match.index + match[0].length) };
    const head = value.slice(0, VERDICT_WITHIN + 16);
    if (/\[\[?[a-z]*\]?$/i.test(head.trimEnd()) && head.length < VERDICT_WITHIN + 16) return { verdict: null, text: value.slice(0, value.search(/\[\[?[a-z]*\]?$/i)) };
    return { verdict: null, text: value };
  }

  function held(text) {
    return String(text).replace(/(?:^|\n)[ \t]*(?:[-*+]|\d{1,9}[.)]?|([a-zA-Z])\1?[.)]?)[ \t]*$/, '');
  }

  const MOST_REPLY_STEPS = 64;

  function timeline(text, steps) {
    const value = String(text || '');
    const parts = [];
    let at = 0;
    for (const raw of (Array.isArray(steps) ? steps : []).slice(0, MOST_REPLY_STEPS)) {
      const item = typeof raw === 'string' ? { text: raw } : raw;
      if (!item || typeof item.text !== 'string' || !item.text) continue;
      let end = Number.isInteger(item.at) ? Math.max(at, Math.min(value.length, item.at)) : at;
      if (end > 0 && end < value.length && /[\uD800-\uDBFF]/.test(value[end - 1]) && /[\uDC00-\uDFFF]/.test(value[end])) end++;
      if (end > at) parts.push({ text: value.slice(at, end) });
      const step = { text: item.text, tool: typeof item.tool === 'string' ? item.tool : '', failed: item.failed === true, found: Array.isArray(item.found) ? item.found.filter((f) => typeof f === 'string').slice(0, 12) : [] };
      if (parts.at(-1)?.steps) parts.at(-1).steps.push(step);
      else parts.push({ steps: [step] });
      at = end;
    }
    if (at < value.length) parts.push({ text: value.slice(at) });
    return parts;
  }

  const openTraces = new WeakSet();
  const OPENING_TOOLS = new Set(['open_note', 'read_document', 'read_transcript', 'read_original']);
  const plural = (count, word) => `${count} ${word}${count === 1 ? '' : 's'}`;

  function traceSummary(steps, usedTitles, changes) {
    const worked = steps.filter((step) => !step.failed);
    const opened = new Set();
    for (const step of worked) {
      if (!OPENING_TOOLS.has(step.tool)) continue;
      opened.add(/.*“(.+?)”/.exec(step.text)?.[1] || step.text);
    }
    const noun = worked.some((step) => step.tool === 'read_document') ? 'source' : 'note';
    const used = usedTitles.filter((title) => opened.has(title)).length;
    let line;
    if (opened.size) line = `Opened ${plural(opened.size, noun)}${used > 0 && used < opened.size ? `, used ${used}` : ''}`;
    else if (worked.some((step) => step.tool === 'search_notes')) line = 'Searched your notes';
    else if (worked.some((step) => step.tool === 'search_chats')) line = 'Searched your chats';
    else line = plural(steps.length, 'step');
    return changes > 0 ? `${line} · made ${plural(changes, 'change')}` : line;
  }

  const DOING = { Read: 'Reading', Searched: 'Searching', Followed: 'Following', Suggested: 'Suggesting', Looked: 'Looking', Opened: 'Opening', Asked: 'Asking', Checked: 'Checking', Calculated: 'Calculating', Worked: 'Working', Planned: 'Planning' };

  function doing(step) {
    const text = String(step && step.text || '');
    const verb = /^(\S+)(.*)$/s.exec(text);
    return step && !step.failed && verb && DOING[verb[1]] ? `${DOING[verb[1]]}${verb[2].replace(/[.…]$/, '')}…` : text;
  }

  function fixedSteps(steps, finished = false) {
    const group = (tool) => (tool === 'ask_user' ? 'quiz' : tool);
    return steps
      .filter((step, n) => !(step && step.failed && steps.slice(n + 1).some((later) => later && group(later.tool) === group(step.tool) && !later.failed)))
      .map((step) => (finished && step && step.failed && group(step.tool) === 'quiz' ? { ...step, text: "Felix couldn't make that question. Ask again." } : step));
  }

  function notFromNotes(steps, shown, used) {
    if (!String(shown).trim() || used > 0 || /beyond your notes/i.test(shown)) return false;
    const worked = steps.filter((step) => !step.failed);
    if (worked.some((step) => OPENING_TOOLS.has(step.tool) || step.found.length)) return false;
    return worked.some((step) => step.tool === 'search_notes');
  }

  const QUIZ_KINDS = { multiple_choice: 'Multiple choice', fill_blank: 'Fill in the blank', free_response: 'Free response' };

  function plainAnswer(text) {
    return String(text || '').toLowerCase().normalize('NFKD').replace(/[\u0300-\u036f]/g, '').replace(/[^\p{L}\p{N}]+/gu, ' ').trim();
  }

  function accepted(quiz) {
    return (quiz.kind === 'fill_blank' ? String(quiz.answer || '').split('|') : [String(quiz.answer || '')]).map((a) => a.trim()).filter(Boolean);
  }

  function checkQuiz(quiz, given) {
    if (!quiz || quiz.kind !== 'multiple_choice') return null;
    const said = plainAnswer(given);
    return Boolean(said) && accepted(quiz).some((a) => plainAnswer(a) === said);
  }

  // stay: the user asked about this answer (Ask Felix why), so Felix explains
  // and stops; the card keeps its Next question button for when they are ready.
  function aside(quiz) {
    const felt = Number.isFinite(quiz.surePct) ? `${quiz.surePct}%` : SURE[quiz.sure] ? SURE[quiz.sure].toLowerCase() : '';
    const how = felt ? ` (how sure I felt: ${felt})` : '';
    const why = quiz.thinking ? ` My thinking: ${clipped(quiz.thinking, MOST_THINKING)}` : '';
    return how + why;
  }

  function quizSay(quiz, given, correct, stay = false) {
    if (quiz.predict) return [`Prediction. Question: ${quiz.question}`, `My prediction: ${given}`, 'This was my attempt before you taught the step, so do not mark it and do not write a verdict. Start from what I predicted, say what is actually true and why, teach this one step, then end with a check using the quiz tool.'].join('\n');
    const next = stay
      ? 'Only explain this answer: do not ask a new practice question in this reply. I will press Next question when I am ready.'
      : 'If we are practising, ask the next question with the quiz tool so it appears as a card, or in a lesson teach the next step and end with one; never ask a practice question in plain text.';
    const right = 'praise me briefly in your own words, only for what my answer and thinking show (a guess is not reasoning), then explain why it is right: the idea behind it, and a quick way to see it or remember it next time';
    const lines = [`Quiz answer. Question: ${quiz.question}`, `My answer: ${given}${aside(quiz)}`];
    if (correct === null) {
      const key = quiz.kind === 'fill_blank' ? `Accepted answers: ${accepted(quiz).join(' or ')}.` : `A model answer to compare with: ${quiz.answer}.`;
      lines.push(`Mark it: start your reply with [[correct]] or [[incorrect]]. Judge what my answer means, not how it is written: the same value or idea in other words or notation counts as right (sin x and sin(x), 1/2 and 0.5, f'(x) and df/dx). Be critical of the reasoning: a right result reached by a wrong or missing step, or an answer too vague to show I understand, is [[incorrect]]. If the question has more than one good answer (design, writing, interpretation), mark me right when I make a defensible case with the relevant principles, and treat the model answer as one strong answer, not the only one. Even if I am right, point out any gap, jump or loose term and what a complete answer would add. ${key} If I am right, ${right}. If I am wrong, I have already tried, so do not give a hint: give the right answer and explain it fully, say where my answer went wrong, and end with an intuitive shortcut or tip for keeping it in mind. ${next}`);
    } else if (correct) {
      lines.push(`leo checked it: right. Start your reply with [[correct]], then ${right}. ${next}`);
    } else {
      lines.push(`leo checked it: wrong; the right answer is ${accepted(quiz)[0] || quiz.answer}. Start your reply with [[incorrect]]. I have already tried, so do not give a hint: explain why the right answer is right, then why each of the other choices is wrong, and end with a quick way to tell them apart next time. ${next}`);
    }
    return lines.join('\n');
  }

  function heldSay(quiz, ending) {
    const then = ending
      ? 'That was the last question of this study session: finish it now with end_session and a short recap.'
      : 'Ask the next question with the quiz tool so it appears as a card, with at most one short sentence before it, unless this is a lesson and the next step needs teaching first.';
    if (quiz.told) return ['Next question, please.', then].join('\n');
    const verdict = quiz.correct === true ? 'right' : `wrong; the right answer is ${accepted(quiz)[0] || quiz.answer}`;
    const shown = quiz.feedback ? 'the explanation and feedback' : 'the explanation';
    return [`Quiz answer. Question: ${quiz.question}`, `My answer: ${quiz.given}${aside(quiz)}`, `leo marked it ${verdict}, and already showed me ${shown}, so do not repeat or re-mark it.`, then].join('\n');
  }

  function hintsOf(quiz) {
    const list = Array.isArray(quiz.hints) ? quiz.hints : [];
    return quiz.hint && typeof quiz.hint === 'object' ? [quiz.hint, ...list] : list;
  }

  function hintSay(quiz) {
    const choices = quiz.kind === 'multiple_choice' && (quiz.options || []).length ? `\nChoices: ${quiz.options.join(' | ')}` : '';
    const given = hintsOf(quiz).map((h) => grade(String(h.text || '')).text.trim()).filter(Boolean);
    const before = given.length ? `\nHints you already gave me:\n${given.map((h, n) => `${n + 1}. ${h}`).join('\n')}\nI still need help, so give the next hint: one step further than those, without repeating them.` : '\nGive me one short hint that helps me work it out myself: point me at the idea or first step.';
    return `Hint, please. Question: ${quiz.question}${choices}${before}\nDo not give the answer, do not say which choice is right or wrong, do not mark anything and do not ask a new question.`;
  }

  const CITED = /\s?\[((?:[nf]\d+(?:\s+[LlPpSs]\d+(?:\s*[-–]\s*\d+)?)?)(?:\s*[,;]\s*[nf]\d+(?:\s+[LlPpSs]\d+(?:\s*[-–]\s*\d+)?)?)*)\]/g;

  function citations(list) {
    return list
      .split(/\s*[,;]\s*/)
      .map((item) => {
        const m = /^([nf])(\d+)(?:\s+([LlPpSs])(\d+)(?:\s*[-–]\s*(\d+))?)?$/.exec(item.trim());
        if (!m) return null;
        const kind = m[3] ? m[3].toUpperCase() : '';
        const from = m[4] ? Number(m[4]) : null;
        const to = m[5] ? Number(m[5]) : from;
        return { id: `${m[1]}${m[2]}`, file: m[1] === 'f', kind, from, to };
      })
      .filter(Boolean);
  }

  function placeWord(c, name) {
    if (c.from === null) return '';
    if (c.kind === 'L') return c.to !== c.from ? `lines ${c.from}–${c.to}` : `line ${c.from}`;
    const slide = /\.pptx?$/i.test(name || '') || c.kind === 'S';
    return `${slide ? 'slide' : 'p.'} ${c.from}`.replace('p. ', 'p.');
  }

  function numbered(text, sources) {
    const byN = new Map((sources || []).map((s) => [`n${s.n}`, s]));
    const out = [];
    for (const m of String(text).matchAll(CITED)) {
      for (const c of citations(m[1])) {
        const s = !c.file && byN.get(c.id);
        if (!s) continue;
        const lines = c.kind === 'L' && c.from !== null ? `${c.from}-${c.to}` : '';
        const key = `${s.id}|${lines}`;
        if (!out.some((x) => x.key === key)) out.push({ key, n: out.length + 1, s, lines, where: lines ? placeWord(c) : '' });
      }
    }
    return out;
  }

  function cite(html, sources, escape, files, marks) {
    const byN = new Map((sources || []).map((s) => [`n${s.n}`, s]));
    const byF = new Map((files || []).map((f) => [`f${f.n}`, f]));
    const short = (t) => (t.length > 28 ? t.slice(0, 27) + '…' : t);
    const chip = (whole, list) => {
      const chips = citations(list)
        .map((c) => {
          if (c.file) {
            const f = byF.get(c.id);
            if (!f) return '';
            const at = c.from === null ? '' : `${c.kind === 'L' ? 'L' : 'p'}${c.from}${c.to !== c.from ? `-${c.to}` : ''}`;
            const where = placeWord(c, f.name);
            return `<button class="cite cite-file" data-chat="original" data-note="${escape(f.note)}" data-name="${escape(f.name)}" data-at="${escape(at)}" title="${escape(`${f.name}${where ? `, ${where}` : ''} (from “${f.title}”)`)}">${escape(short(f.name))}${where ? `<span class="cite-at">${escape(where)}</span>` : ''}</button>`;
          }
          const s = byN.get(c.id);
          if (!s) return '';
          const lines = c.kind === 'L' && c.from !== null ? `${c.from}-${c.to}` : '';
          const where = lines ? placeWord(c) : '';
          const mark = marks && marks.find((x) => x.key === `${s.id}|${lines}`);
          if (mark) return `<button class="cite cite-n" data-chat="open" data-id="${escape(s.id)}"${lines ? ` data-line="${lines}"` : ''} title="${escape(`${s.title}${where ? `, ${where}` : ''}`)}" aria-label="${escape(`Source ${mark.n}: ${s.title}${where ? `, ${where}` : ''}`)}">${mark.n}</button>`;
          return `<button class="cite" data-chat="open" data-id="${escape(s.id)}"${lines ? ` data-line="${lines}"` : ''} title="${escape(`${s.title}${where ? `, ${where}` : ''}`)}">${escape(short(s.title))}${where ? `<span class="cite-at">${escape(where)}</span>` : ''}</button>`;
        })
        .filter(Boolean);
      return chips.length ? `${whole.startsWith(' ') && !chips.every((c) => c.startsWith('<button class="cite cite-n"')) ? ' ' : ''}${chips.join('')}` : '';
    };
    const skipped = [];
    return html.replace(/<[^>]*>|[^<]+/g, (part) => {
      if (part[0] !== '<') return skipped.length ? part : part.replace(CITED, chip);
      const tag = /^<(\/?)([a-z0-9]+)/i.exec(part);
      if (!tag) return part;
      const name = tag[2].toLowerCase();
      if (tag[1]) {
        if (skipped.at(-1) === name) skipped.pop();
      } else if (!part.endsWith('/>') && (skipped.length ? /^(?:code|pre|span|div)$/.test(name) : /^(?:code|pre)$/.test(name) || /^<(?:span|div) class="math/i.test(part))) skipped.push(name);
      return part;
    });
  }

  function cited(text, sources) {
    const used = new Set();
    for (const m of String(text).matchAll(CITED)) for (const c of citations(m[1])) if (!c.file) used.add(c.id);
    return (sources || []).filter((s) => used.has(`n${s.n}`));
  }

  const THUMB = /^data:image\/(?:jpeg|png);base64,[A-Za-z0-9+/=]+$/;
  const glyph = (paths) => `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${paths}</svg>`;
  const PAGE = '<path d="M6 3h8l4 4v14H6z"/><path d="M14 3v4h4"/>';
  const FILE_GLYPHS = {
    pdf: glyph(`${PAGE}<path d="M9 13h6M9 17h4"/>`),
    doc: glyph(`${PAGE}<path d="M9 11h6M9 14h6M9 17h4"/>`),
    slides: glyph('<rect x="3" y="4" width="18" height="12" rx="2"/><path d="M12 16v4M8 20h8M7 9h6"/>'),
    sheet: glyph('<rect x="4" y="4" width="16" height="16" rx="2"/><path d="M4 10h16M4 15h16M10 4v16"/>'),
    image: glyph('<rect x="3" y="4" width="18" height="16" rx="2"/><circle cx="9" cy="10" r="2"/><path d="M21 16l-5-5-9 9"/>'),
    text: glyph(`${PAGE}<path d="M9 12h6M9 16h6"/>`),
    other: glyph(PAGE),
    note: glyph('<path d="M6 3h9l4 4v14H6z"/><path d="M9 12h7M9 16h5"/>'),
  };

  function noteExcerpt(title, body) {
    const lines = String(body || '').replace(/```[\s\S]*?```/g, '').split('\n')
      .map((line) => line.replace(/^\s*(#{1,6}\s+|[-*+]\s+(\[[ xX]\]\s+)?|\d+[.)]\s+|>\s?)/, '').replace(/\[\[|\]\]|[*_`~]/g, '').trim())
      .filter(Boolean);
    return [title || 'Untitled', ...lines.slice(0, 8)].join('\n').slice(0, 320);
  }

  function noteCard(r, escape, { excerpt = '', remove = false } = {}) {
    const title = r.title || 'Untitled';
    const peek = excerpt ? `<span class="file-page">${escape(excerpt)}</span>` : `<span class="file-glyph">${FILE_GLYPHS.note}</span>`;
    const x = remove ? `<button type="button" class="file-x" data-chat="unref" data-id="${escape(r.id)}" aria-label="Remove ${escape(title)}">×</button>` : '';
    return `<span class="file-card note-card tone-note" title="${escape(title)}"><button type="button" class="file-open" data-chat="open" data-id="${escape(r.id)}" aria-label="Open ${escape(title)}"><span class="file-peek">${peek}</span><span class="file-foot"><span class="file-badge">${FILE_GLYPHS.note}Note</span><span class="file-name">${escape(title)}</span></span></button>${x}</span>`;
  }

  function fileKind(name) {
    const found = String(name || '').match(/\.([a-z0-9]{1,8})$/i);
    const ext = found ? found[1].toLowerCase() : '';
    const tone = ext === 'pdf' ? 'pdf'
      : /^(docx?|rtf|odt|pages)$/.test(ext) ? 'doc'
        : /^(pptx?|key|odp)$/.test(ext) ? 'slides'
          : /^(xlsx?|csv|numbers|ods)$/.test(ext) ? 'sheet'
            : /^(png|jpe?g|gif|webp|heic|heif|bmp|tiff?|avif)$/.test(ext) ? 'image'
              : /^(md|markdown|txt|text)$/.test(ext) ? 'text' : 'other';
    return { label: ext ? (ext === 'jpeg' ? 'JPG' : ext.toUpperCase()) : 'FILE', tone };
  }

  function fileCard(f, escape, { remove = '', reading = false } = {}) {
    const kind = fileKind(f.name);
    const thumb = f.thumb && THUMB.test(f.thumb) ? f.thumb : '';
    const excerpt = typeof f.excerpt === 'string' ? f.excerpt.trim() : '';
    const peek = thumb
      ? `<img src="${escape(thumb)}" alt="">`
      : excerpt
        ? `<span class="file-page">${escape(excerpt)}</span>`
        : `<span class="file-glyph">${FILE_GLYPHS[kind.tone]}</span>`;
    const busy = reading ? '<span class="file-reading">Reading…</span>' : '';
    const x = remove ? `<button class="file-x" data-chat="unfile" data-id="${escape(remove)}" aria-label="Remove ${escape(f.name)}">×</button>` : '';
    return `<span class="file-card tone-${kind.tone}${reading ? ' reading' : ''}" title="${escape(f.name)}"><span class="file-peek">${peek}${busy}</span><span class="file-foot"><span class="file-badge">${FILE_GLYPHS[kind.tone]}${escape(kind.label)}</span><span class="file-name">${escape(f.name)}</span></span>${x}</span>`;
  }

  function pastedNames(files) {
    return [...files].map((f, i) => (f.name && !/^image\.\w+$/i.test(f.name)
      ? f
      : new File([f], `pasted-${i + 1}.${(f.type.split('/')[1] || 'png').replace('jpeg', 'jpg')}`, { type: f.type })));
  }

  function money(cost) {
    if (cost >= 0.995) return `$${cost.toFixed(2)}`;
    if (cost >= 0.01) return `$${cost.toFixed(3)}`;
    if (cost >= 0.0001) return `$${cost.toFixed(4)}`;
    return 'under $0.0001';
  }

  const USAGE_KINDS = { answer: 'Answers', tool_continuation: 'After tool reads', retry: 'Recovery attempts', study_grading: 'Study grading checks', summary: 'Conversation summaries', other_writing: 'Other writing · graph and notes', quick_review: 'Quick checks', margin_read: 'Reading along' };

  function usageCount(value) {
    const n = Number(value);
    return Number.isFinite(n) && n > 0 ? Math.floor(n) : 0;
  }

  function usageKinds(rows) {
    if (!Array.isArray(rows)) return [];
    return rows.filter((row) => row && Object.hasOwn(USAGE_KINDS, row.purpose)).map((row) => ({
      purpose: row.purpose,
      label: USAGE_KINDS[row.purpose],
      calls: usageCount(row.calls),
      failures: usageCount(row.failures),
      unreported: usageCount(row.unreported),
      estimated: usageCount(row.estimated),
      median_ms: usageCount(row.median_ms),
      p95_ms: usageCount(row.p95_ms),
    }));
  }

  function usageBack(offset, today) {
    const [date, hour] = String(today).split(' ');
    const [y, m, d] = date.split('-').map(Number);
    if (hour === undefined) return new Date(Date.UTC(y, m - 1, d - offset)).toISOString().slice(0, 10);
    return new Date(Date.UTC(y, m - 1, d, Number(hour) - offset)).toISOString().slice(0, 13).replace('T', ' ');
  }

  function usagePeriod(days, span, today) {
    const length = [1, 7, 30, 90].includes(span) ? span : 30;
    const units = length === 1 ? 24 : length;
    const ok = (length === 1 ? /^\d{4}-\d{2}-\d{2} \d{2}$/ : /^\d{4}-\d{2}-\d{2}$/).test(String(today)) && !Number.isNaN(Date.parse(String(today).slice(0, 10)));
    const first = ok ? usageBack(units - 1, today) : '';
    const pick = (list, allowed) => (Array.isArray(list) ? list : []).filter((row) => ok && row && typeof row.day === 'string' && typeof row.name === 'string' && row.day >= first && row.day <= today && allowed(row.name)).map((row) => {
      const input = usageCount(row.input);
      return { day: row.day, name: row.name, calls: usageCount(row.calls), input, cached: Math.min(input, usageCount(row.cached)), output: usageCount(row.output) };
    });
    const models = pick(days && days.models, () => true);
    const kinds = pick(days && days.kinds, (name) => Object.hasOwn(USAGE_KINDS, name));
    const totals = { calls: 0, input: 0, cached: 0, output: 0, share: 0 };
    for (const row of models) {
      totals.calls += row.calls;
      totals.input += row.input;
      totals.cached += row.cached;
      totals.output += row.output;
    }
    totals.share = totals.input ? totals.cached / totals.input : 0;
    const size = length === 90 ? 7 : 1;
    const count = Math.ceil(units / size);
    const bars = ok ? Array.from({ length: count }, (_, b) => {
      const ago = count - 1 - b;
      const from = usageBack(Math.min(units - 1, ago * size + size - 1), today);
      const to = usageBack(ago * size, today);
      const by = new Map();
      const bar = { from, to, calls: 0, input: 0, output: 0, by: [] };
      for (const row of models) {
        if (row.day < from || row.day > to) continue;
        bar.calls += row.calls;
        bar.input += row.input;
        bar.output += row.output;
        by.set(row.name, (by.get(row.name) || 0) + row.input + row.output);
      }
      bar.by = [...by].filter(([, tokens]) => tokens > 0);
      return bar;
    }) : [];
    const group = (rows, measures) => {
      const sums = new Map();
      for (const row of rows) {
        const held = sums.get(row.name) || { name: row.name, calls: 0, input: 0, output: 0, tokens: 0 };
        held.calls += row.calls;
        held.input += row.input;
        held.output += row.output;
        held.tokens += row.input + row.output;
        sums.set(row.name, held);
      }
      return [...sums.values()].sort((a, b) => b[measures] - a[measures] || (a.name < b.name ? -1 : 1));
    };
    return { length, totals, bars, models: group(models, 'tokens'), kinds: group(kinds, 'calls') };
  }

  function usageRank(days, today) {
    const week = usagePeriod(days, 7, today);
    const all = usagePeriod(days, 90, today);
    const order = (key, list, ofWeek) => {
      const recent = new Map(ofWeek.map((row) => [row.name, row[key]]));
      return list.slice().sort((a, b) => (recent.get(b.name) || 0) - (recent.get(a.name) || 0) || b[key] - a[key] || (a.name < b.name ? -1 : 1)).map((row) => row.name);
    };
    return { models: order('tokens', all.models, week.models), kinds: order('calls', all.kinds, week.kinds) };
  }

  function usageFold(items, keep) {
    const rows = items.filter((item) => item.value > 0);
    const shown = rows.filter((item) => keep.includes(item.name)).sort((a, b) => b.value - a.value).map((item) => ({ ...item, rank: keep.indexOf(item.name) }));
    const rest = rows.filter((item) => !keep.includes(item.name));
    if (rest.length) shown.push({ name: 'Other', other: true, rank: -1, value: rest.reduce((sum, item) => sum + item.value, 0), calls: rest.reduce((sum, item) => sum + (item.calls || 0), 0) });
    return shown;
  }

  function spentLabel(spent) {
    if (!spent || typeof spent !== 'object' || typeof spent.by !== 'string') return null;
    const model = typeof spent.model === 'string' && spent.model ? spent.model : '';
    const effort = typeof spent.effort === 'string' && spent.effort ? `${spent.effort} effort` : '';
    const about = spent.estimated ? 'about ' : '';
    const steps = spent.steps > 1 ? ` over ${spent.steps} steps` : '';
    const cached = Number(spent.cached || 0) > 0 ? ` (${Number(spent.cached).toLocaleString()} from the cache)` : '';
    const title = `${about}${Number(spent.input || 0).toLocaleString()} tokens in${cached}, ${Number(spent.output || 0).toLocaleString()} out${steps}`;
    let parts;
    const by = spent.auto === true ? `Auto · ${spent.by}` : spent.by;
    if (spent.plan) parts = [by, model, effort || 'default effort', 'on your plan'];
    else if (spent.local) parts = [by, model, 'free on this computer'];
    else if (typeof spent.cost === 'number') parts = [`${spent.estimated ? '≈ ' : ''}${money(spent.cost)}`, by, model];
    else parts = [by, model, 'price unknown'];
    return { text: parts.filter(Boolean).join(' · '), title };
  }

  function linkCitations(text, sources) {
    const byN = new Map((sources || []).map((s) => [`n${s.n}`, s]));
    return String(text || '').replace(/\[(n\d+(?:\s*,\s*n\d+)*)(?:\s+L\d+(?:-\d+)?)?\]/g, (whole, list) => {
      const links = list
        .split(/\s*,\s*/)
        .map((n) => byN.get(n))
        .filter(Boolean)
        .map((s) => `[[${s.title.replace(/[[\]|]/g, '')}]]`);
      return links.length ? links.join(' ') : whole;
    });
  }

  function squash(text) {
    return String(text || '').toLowerCase().replace(/[^\p{L}\p{N}]+/gu, '');
  }

  function wholeBlocks(lines, a, b) {
    let fence = -1;
    let math = -1;
    const opens = [];
    for (let k = 0; k < lines.length; k++) {
      const line = lines[k].trim();
      if (math < 0 && /^(```|~~~)/.test(line)) {
        if (fence < 0) fence = k;
        else {
          opens.push([fence, k]);
          fence = -1;
        }
      } else if (fence < 0 && line === '$$') {
        if (math < 0) math = k;
        else {
          opens.push([math, k]);
          math = -1;
        }
      }
    }
    for (const [start, end] of opens) {
      if (a > start && a <= end) a = start;
      if (b >= start && b < end) b = end;
    }
    return [a, b];
  }

  function answerLines(source, picked) {
    const lines = String(source || '').split('\n');
    const want = squash(picked);
    if (!want) return '';
    let joined = '';
    const ends = [];
    for (const line of lines) {
      joined += squash(line);
      ends.push(joined.length);
    }
    const lineAt = (pos) => ends.findIndex((e) => e > pos);
    let from = joined.indexOf(want);
    let to = from + want.length - 1;
    if (from < 0) {
      const ends = Math.max(4, Math.min(16, Math.floor(want.length / 2)));
      let start = -1;
      for (let k = 0; k + ends <= want.length && k <= 200 && start < 0; k++) start = joined.indexOf(want.slice(k, k + ends));
      if (start < 0) return String(picked).trim();
      let end = -1;
      for (let k = want.length; k - ends >= 0 && k >= want.length - 200 && end < 0; k--) {
        const tail = want.slice(k - ends, k);
        const at = joined.indexOf(tail, start);
        if (at >= 0) end = at + tail.length - 1;
      }
      from = start;
      to = end < 0 ? start + ends - 1 : end;
    }
    const [a, b] = wholeBlocks(lines, lineAt(from), lineAt(to));
    return lines.slice(a, b + 1).join('\n').trim();
  }

  function asNote(question, text, sources) {
    const answer = linkCitations(grade(String(text || '')).text, sources).trim();
    const asked = String(question || '').replace(/\s+/g, ' ').trim();
    let title = asked;
    if (title.length > 70) {
      const cut = title.slice(0, 70);
      title = `${cut.slice(0, cut.lastIndexOf(' ') > 30 ? cut.lastIndexOf(' ') : 70)}…`;
    }
    return { title: title || 'From Felix', body: asked ? `**Q:** ${asked}\n\n${answer}` : answer };
  }

  function reviewPrompt(items) {
    const lines = items.map((m, i) => `${i + 1}. ${m.question}${m.answer ? ` (last time I said: ${m.answer})` : ''}`);
    return `Let's review questions I got wrong before. Test each idea again in a new situation, one at a time, and wait for my answer:\n${lines.join('\n')}`;
  }

  function mentionAt(text, caret) {
    const before = text.slice(0, caret);
    const at = before.lastIndexOf('@');
    if (at < 0 || (at > 0 && !/\s/.test(before[at - 1]))) return null;
    const query = before.slice(at + 1);
    if (/\n/.test(query) || query.length > 40 || /^\s/.test(query)) return null;
    return { start: at, query };
  }

  function recentNotes(messages, most = 6) {
    const out = [];
    for (const m of [...messages].reverse().filter((x) => x.role === 'assistant').slice(0, 3)) {
      const opened = (m.sources || []).filter((s) => s && /^(opened|changed) by Felix|its pictures looked at/.test(String(s.why || '')));
      for (const s of [...cited(m.text || '', m.sources || []), ...opened]) {
        if (s && s.id && !out.includes(s.id) && out.length < most) out.push(s.id);
      }
    }
    return out;
  }

  const clipped = (text, most) => {
    const flat = String(text || '').replace(/\s+/g, ' ').trim();
    return flat.length > most ? `${flat.slice(0, most - 1)}…` : flat;
  };

  const MOST_IDEAS = 12;
  const MOST_SESSION_QUESTIONS = 30;

  function startSession(ideas) {
    const list = (Array.isArray(ideas) ? ideas : []).filter((i) => typeof i === 'string' && i.trim()).slice(0, MOST_IDEAS).map((i) => i.trim().slice(0, 80));
    return { ideas: list, outcomes: list.map(() => ({ state: 'open', misses: 0, right: 0 })), asked: 0, state: 'active' };
  }

  function scoreSession(session, quiz, correct) {
    const idea = quiz && quiz.idea;
    if (!session || session.state !== 'active' || !idea || correct === null || correct === undefined || quiz.scored) return session;
    const keepOpen = Boolean(quiz.more || quiz.sure === 'guess' || hintsOf(quiz).length);
    const outcome = session.outcomes[idea - 1];
    if (!outcome || outcome.state !== 'open') return session;
    quiz.scored = true;
    if (correct === false) outcome.misses += 1;
    if (!keepOpen && correct === true) outcome.right = (outcome.right || 0) + 1;
    if (outcome.misses >= 2) outcome.state = 'review';
    else if (outcome.right >= 2) outcome.state = outcome.misses ? 'done' : 'first';
    if (session.outcomes.every((o) => o.state !== 'open') || session.asked >= MOST_SESSION_QUESTIONS) session.state = 'finishing';
    return session;
  }

  function sessionBody(session) {
    if (!session || (session.state !== 'active' && session.state !== 'finishing')) return null;
    return { ideas: session.ideas, closed: session.outcomes.map((o) => o.state !== 'open'), asked: session.asked, right: session.outcomes.map((o) => o.right || 0), misses: session.outcomes.map((o) => o.misses || 0) };
  }

  function sessionSummary(session) {
    const pick = (test) => session.ideas.filter((_, i) => test(session.outcomes[i]));
    return {
      total: session.ideas.length,
      covered: session.outcomes.filter((o) => o.state !== 'open').length,
      first: pick((o) => o.state === 'first').length,
      review: pick((o) => o.state === 'review'),
      once: pick((o) => o.state === 'open' && o.right > 0),
      skipped: pick((o) => o.state === 'open' && !o.right),
    };
  }

  function passageLine(p) {
    if (!p || typeof p.text !== 'string') return '';
    const lines = p.from ? ` lines ${p.from}${p.to > p.from ? `–${p.to}` : ''}` : '';
    return `[Selected passage from “${clipped(p.title || 'a note', 80)}”${lines}: “${clipped(p.text, 300)}”]`;
  }

  function historyText(m, canonical = false) {
    if (m.role === 'user') return [passageLine(m.selection), m.say || m.text].filter(Boolean).join('\n');
    const lines = [];
    const by = m.spent && typeof m.spent.by === 'string' ? [m.spent.by, m.spent.model].filter(Boolean).join(' · ') : '';
    if (by) lines.push(`[Written by ${by}]`);
    const steps = (m.steps || []).map((s) => (typeof s === 'string' ? { text: s, found: [] } : s)).filter((s) => s && s.text);
    if (steps.length) {
      lines.push(`[What Felix did for this answer: ${steps.slice(0, 12).map((s) => {
        const found = (s.found || []).filter((f) => typeof f === 'string' && f !== s.text).slice(0, 5);
        return clipped(s.text, 120) + (found.length ? ` (found: ${found.map((f) => clipped(f, 60)).join(', ')})` : '');
      }).join('; ')}]`);
    }
    for (const a of m.asks || []) {
      lines.push(`[Felix asked: ${clipped(a.question, 300)}${(a.options || []).length ? ` (choices: ${a.options.join(' | ')})` : ''}${typeof a.answered === 'string' ? `; the user answered: ${clipped(a.answered, 200)}` : ''}]`);
    }
    for (const q of m.quizzes || []) {
      const how = typeof q.given !== 'string' ? 'not answered yet' : q.correct === true ? `answered “${clipped(q.given, 120)}”, right${hintsOf(q).length ? ' after hints' : ''}` : q.correct === false ? `answered “${clipped(q.given, 120)}”, wrong` : `answered “${clipped(q.given, 200)}”`;
      const replied = !canonical && q.reply && q.reply.text ? `; Felix replied: ${clipped(grade(q.reply.text).text, 400)}` : '';
      lines.push(`[${q.predict ? 'Prediction' : 'Practice'} question (${(QUIZ_KINDS[q.kind] || 'question').toLowerCase()}): ${clipped(q.question, 300)}${(q.options || []).length ? `; choices: ${q.options.join(' | ')}` : ''}; answer: ${clipped(q.answer, 200)}${canonical ? '' : `; ${how}${replied}`}]`);
    }
    for (const p of m.proposals || []) {
      const state = p.state === 'applied' ? 'applied' : p.state === 'dismissed' ? 'dismissed by the user' : 'waiting for the user';
      const what = p.kind === 'create' ? `Suggested a new note “${clipped(p.title, 80)}”` : p.kind === 'edit' ? `Suggested a change to “${clipped(p.title, 80)}”: “${clipped(p.find, 80)}” → “${clipped(p.replace, 120)}”` : `Suggested: ${clipped(p.summary, 160)}`;
      lines.push(`[${what}; ${state}]`);
    }
    lines.push(canonical ? m.text || '' : grade(m.text || '').text || m.text || '');
    return lines.filter(Boolean).join('\n');
  }

  const MOST_MODEL_TURNS = 400;
  const MOST_QUIZ_HISTORY = 32;
  const MOST_HISTORY_DEPTH = 4;
  const MOST_NESTED_TURNS = 128;
  const MOST_HISTORY_REQUEST = 16000;

  function historyRequest(value) {
    const chars = [];
    for (const char of String(value || '')) { if (chars.length >= MOST_HISTORY_REQUEST) break; chars.push(char); }
    return chars.join('');
  }

  function modelHistory(messages) {
    const turns = [];
    const add = (role, text) => {
      if (!text) return;
      if (role === 'assistant' && turns.at(-1)?.role === role) turns.at(-1).text += `\n\n${text}`;
      else turns.push({ role, text });
      if (turns.length > MOST_MODEL_TURNS) turns.shift();
    };
    const visit = (message, depth, left) => {
      if (!message || depth > MOST_HISTORY_DEPTH || left.count <= 0 || message.pending || message.error || message.steer === 'waiting') return;
      left.count -= 1;
      if (!['user', 'assistant'].includes(message.role)) return;
      add(message.role, historyText(message, true));
      if (message.role !== 'assistant') return;
      for (const quiz of (message.quizzes || []).slice(0, MOST_QUIZ_HISTORY)) {
        const hints = hintsOf(quiz).slice(-MOST_QUIZ_HISTORY);
        const interaction = (reply, request) => {
          if (!reply || reply.pending || left.count <= 0) return;
          add('user', typeof reply.request === 'string' ? historyRequest(reply.request) : historyRequest(request));
          visit(reply, depth + 1, left);
        };
        for (let i = 0; i < hints.length; i += 1) interaction(hints[i], hintSay({ ...quiz, hint: null, hints: hints.slice(0, i) }));
        if (typeof quiz.given === 'string') interaction(quiz.reply, quizSay(quiz, quiz.given, checkQuiz(quiz, quiz.given)));
      }
    };
    for (const message of messages.slice(-MOST_MODEL_TURNS)) visit(message, 0, { count: MOST_NESTED_TURNS });
    return turns;
  }

  function threadRefs(messages) {
    const out = [];
    for (const m of [...messages].reverse()) {
      if (m.role !== 'user' || !Array.isArray(m.refs)) continue;
      for (const r of m.refs) {
        if (r && r.id && !out.some((x) => x.id === r.id) && out.length < MOST_REFS) out.push(r);
      }
    }
    return out;
  }

  function addRef(refs, note) {
    if (!note || !note.id || refs.some((r) => r.id === note.id) || refs.length >= MOST_REFS) return refs;
    return [...refs, { id: note.id, title: note.title || 'Untitled' }];
  }

  function newId() {
    if (root.crypto && typeof root.crypto.randomUUID === 'function') return root.crypto.randomUUID();
    return `chat-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
  }

  function load(storage) {
    const fresh = { id: null, messages: [], refs: [] };
    try {
      const saved = JSON.parse(storage.getItem(KEY) || 'null');
      if (saved && Array.isArray(saved.messages)) return { id: typeof saved.id === 'string' ? saved.id : null, messages: saved.messages, refs: Array.isArray(saved.refs) ? saved.refs.slice(0, MOST_REFS) : [] };
    } catch (e) {
      return fresh;
    }
    return fresh;
  }

  function save(storage, messages, refs = [], id = null) {
    try {
      storage.setItem(KEY, JSON.stringify({ id, messages: messages.slice(-MOST_KEPT), refs }));
    } catch (e) {
      return;
    }
  }

  function groups(list, now = new Date()) {
    const start = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime();
    const day = 86400000;
    const order = ['Today', 'Yesterday', 'Previous 7 days', 'Previous 30 days', 'Older'];
    const out = new Map(order.map((name) => [name, []]));
    for (const chat of list) {
      const t = new Date(chat.updated_at).getTime();
      const name = t >= start ? 'Today' : t >= start - day ? 'Yesterday' : t >= start - 7 * day ? 'Previous 7 days' : t >= start - 30 * day ? 'Previous 30 days' : 'Older';
      out.get(name).push(chat);
    }
    return order.filter((name) => out.get(name).length).map((name) => ({ name, chats: out.get(name) }));
  }

  function starterWords(starter, picked) {
    if (starter.needs !== 'classes' || !picked.length) return starter.text;
    const list = picked.length === 1 ? picked[0] : `${picked.slice(0, -1).join(', ')} and ${picked[picked.length - 1]}`;
    return `${starter.text} for ${list}`;
  }

  const MOST_FILES = 10;

  function splitFiles(docs, messages) {
    const ids = new Set();
    const names = new Set();
    for (const m of messages || []) {
      if (m.role !== 'user') continue;
      for (const id of m.files || []) ids.add(id);
      if (!m.files) for (const name of m.docs || []) names.add(name);
    }
    const sent = [];
    const waiting = [];
    for (const d of docs || []) (ids.has(d.id) || names.has(d.name) ? sent : waiting).push(d);
    return { sent, waiting };
  }
  const FILE_TYPES = '.pdf,.docx,.pptx,.txt,.md,image/*';
  const NOTE_DRAG = 'application/x-leo-note';

  const WORDS = /\s*\S+|\s+/g;

  const greeting = (text, escape) => (text.match(WORDS) || []).map((part, i) => `<span class="word" style="animation-delay:${160 + Math.min(i, 14) * 30}ms">${escape(part)}</span>`).join('');

  const isWord = (n) => n.nodeType === 1 && n.classList.contains('word');

  function runs(parent) {
    const out = [];
    for (const n of parent.childNodes) {
      const text = n.nodeType === 3 || isWord(n);
      const last = out[out.length - 1];
      if (text && last && last.run) last.nodes.push(n);
      else out.push({ run: text, nodes: [n] });
    }
    return out;
  }

  const said = (item) => item.nodes.map((n) => n.textContent).join('');

  function match(from, to) {
    for (const { name } of [...from.attributes]) if (!to.hasAttribute(name)) from.removeAttribute(name);
    for (const { name, value } of to.attributes) if (from.getAttribute(name) !== value) from.setAttribute(name, value);
  }

  function settle(from, to) {
    const was = runs(from);
    const now = runs(to);
    now.forEach((b, i) => {
      const a = was[i];
      if (!a) {
        for (const n of b.nodes) from.append(n);
        return;
      }
      if (a.run && b.run) {
        const before = said(a);
        const after = said(b);
        if (before === after) return;
        const end = a.nodes[a.nodes.length - 1];
        if (before && after.startsWith(before) && end.nodeType === 3) end.appendData(after.slice(before.length));
        else {
          end.after(...b.nodes);
          a.nodes.forEach((n) => n.remove());
        }
        return;
      }
      const x = a.nodes[0];
      const y = b.nodes[0];
      if (!a.run && !b.run && x.nodeType === 1 && y.nodeType === 1 && x.tagName === y.tagName) {
        if (x.outerHTML === y.outerHTML) return;
        match(x, y);
        settle(x, y);
        return;
      }
      x.before(...b.nodes);
      a.nodes.forEach((n) => n.remove());
    });
    for (const a of was.slice(now.length)) a.nodes.forEach((n) => n.remove());
  }

  const CHAT_LEAVE_MS = 160;
  const PERMISSION_MS = 5000;
  const HISTORY_SIDE_WIDTH = 560;

  const retiredPractice = (p) => typeof p?.kind === 'string' && p.kind.includes('interview');

  function create({ render, escape, icons = {}, drawings = {}, onOpen = () => {}, onCite = () => {}, onOriginal = () => {}, storage = root.localStorage, prepare = null, notify = () => {}, onSaved = () => {}, onChanged = () => {}, onUndone = () => {}, onToggle = () => {}, onAdd = () => {}, onOrganized = () => {}, onLocked = () => {} }) {
    const request = async (url, init = {}) => {
      const response = await fetch(url, { credentials: 'same-origin', ...init });
      if (response.status === 401) onLocked();
      return response;
    };
    const saved = load(storage);
    const stepChevron = (icons.chevron || '').replace('<svg ', '<svg class="msg-step-chevron" ');
    const state = { open: false, id: saved.id || newId(), messages: saved.messages, refs: saved.refs, context: null, dropped: null, fresh: false, busy: null, streak: 0, pick: null, chats: null, sidebar: null, doomed: null, asking: null, files: [], sent: [], filesFor: null, review: [], drafts: {}, found: null, selection: null };
    const panel = document.createElement('aside');
    panel.className = 'chat';
    panel.id = 'chat';
    panel.hidden = true;
    panel.setAttribute('aria-label', 'Ask Felix');
    panel.innerHTML = `
      <nav class="chat-history" id="chat-history" aria-label="Your chats">
        <div class="chat-history-head"><button class="icon-btn chat-history-back" data-chat="history" aria-label="Back to the chat"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M15 18l-6-6 6-6"/></svg></button><b>Chats</b>${NEW_CHAT}</div>
        <div class="chat-history-find"><input id="chat-history-search" type="search" placeholder="Search your chats" autocomplete="off" aria-label="Search your chats"></div>
        <div class="chat-history-list" id="chat-history-list"></div>
      </nav>
      <section class="chat-main">
      <header class="chat-head">
        <button class="icon-btn chat-history-toggle" data-chat="history" aria-label="Your chats" title="Your chats" aria-expanded="false" aria-controls="chat-history"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="M9 4v16"/></svg></button>
        <span class="chat-title quiet" id="chat-title">New chat</span>
        ${NEW_CHAT}
        <button class="icon-btn" data-chat="close" aria-label="Close"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><path d="M6 6l12 12M18 6L6 18"/></svg></button>
      </header>
      <div class="study-progress" id="study-progress" hidden></div>
      <div class="chat-body" id="chat-body" aria-live="polite"></div>
      <footer class="chat-foot">
        <div class="chat-refs" id="chat-refs"></div>
        <form class="chat-compose" id="chat-form">
          <span class="chat-perch" id="chat-face">${felix(50, 'idle')}</span>
          <div class="chat-pick" id="chat-pick" hidden></div>
          <div class="chat-attach-menu" id="chat-attach-menu" role="menu" hidden><button type="button" role="menuitem" data-chat="attach-note">${icons.note || ''}<span>A note from leo</span></button>${prepare ? `<button type="button" role="menuitem" data-chat="attach-file">${icons.paperclip || ''}<span>A file from this device</span></button>` : ''}</div>
          <input type="file" id="chat-file" multiple accept="${FILE_TYPES}" hidden>
          <textarea id="chat-input" rows="1" placeholder="Message Felix, or type @ to add a note…" enterkeyhint="send"></textarea>
          <div class="chat-tools">
            <button class="chat-attach" type="button" data-chat="attach" aria-haspopup="menu" aria-controls="chat-attach-menu" aria-expanded="false" aria-label="Add a note or a file" title="Add a note or a file to the conversation (or type @ for a note)"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M21 11l-8.5 8.5a5 5 0 0 1-7-7L14 4a3.5 3.5 0 0 1 5 5l-8.5 8.5a2 2 0 0 1-3-3L15 7"/></svg></button>
            <button type="button" class="chat-chip" data-chat="drop" hidden><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"></svg><span></span></button>
            <button type="button" class="chat-access" id="chat-access" data-chat="access" aria-haspopup="menu" aria-expanded="false"></button>
            <button class="chat-send" id="chat-send" type="submit" aria-label="Send"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 19V5M6 11l6-6 6 6"/></svg></button>
          </div>
          <div class="chat-access-menu" id="chat-access-menu" role="menu" hidden>${ACCESS.map((a) => `<button type="button" role="menuitemradio" data-chat="access-pick" data-access="${a.id}"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${ACCESS_GLYPHS[a.id]}</svg><span><b>${escape(a.label)}</b><small>${escape(a.hint)}</small></span></button>`).join('')}</div>
        </form>
      </footer>
      <div class="chat-below" id="chat-below"></div>
      </section>`;
    document.body.appendChild(panel);
    const $ = (sel) => panel.querySelector(sel);
    const body = $('#chat-body');
    const input = $('#chat-input');

    function popMenu(menu, open, trigger, then) {
      const motion = root.leoMotion;
      if (!motion) {
        menu.hidden = !open;
        if (then) then();
      } else if (open) motion.unfold(menu, trigger);
      else motion.fold(menu, then);
    }

    function attachMenu(open) {
      const trigger = $('[data-chat="attach"]');
      popMenu($('#chat-attach-menu'), open, trigger);
      trigger.setAttribute('aria-expanded', String(open));
    }

    function accessMenu(open) {
      const trigger = $('#chat-access');
      popMenu($('#chat-access-menu'), open, trigger);
      trigger.setAttribute('aria-expanded', String(open));
    }

    const faces = () => [...document.querySelectorAll('.felix:not(.felix-preview)')];
    const onScreen = () => document.visibilityState !== 'hidden' && root.__LEO_VISIBLE__ !== false;
    let blinkTimer = 0;
    function blink() {
      clearTimeout(blinkTimer);
      if (!onScreen()) return;
      blinkTimer = setTimeout(() => {
        for (const f of faces()) {
          f.classList.add('blink');
          setTimeout(() => f.classList.remove('blink'), 200);
        }
        blink();
      }, 2200 + Math.random() * 3600);
    }
    function mood(name, ms) {
      for (const f of faces()) {
        f.classList.remove('dance', 'droop', 'wave', 'cheer', 'nod', 'wake', 'perk', 'monster', 'sweet', 'grow-down', ...TAPS, ...DEMON_TAPS);
        const big = name === 'monster' || name === 'sweet';
        const box = f.getBoundingClientRect();
        if (big && box.top < box.height * 2.5) f.classList.add('grow-down');
        f.classList.add(name);
        setTimeout(() => f.classList.remove(name, ...(big ? ['grow-down'] : [])), ms);
      }
    }
    const working = new WeakMap();
    const timed = new WeakSet();
    function tookFor(ms) {
      const secs = Math.max(0, Math.round(ms / 1000));
      return secs >= 60 ? `${Math.floor(secs / 60)}:${String(secs % 60).padStart(2, '0')}` : `${secs} s`;
    }
    function workingFor(since) {
      return `Working · ${tookFor(Date.now() - since)}`;
    }
    let ticking = 0;
    function tick() {
      if (ticking) return;
      ticking = setInterval(() => {
        const clocks = document.querySelectorAll('.msg-clock[data-since]');
        if (!clocks.length) {
          clearInterval(ticking);
          ticking = 0;
          return;
        }
        for (const el of clocks) el.textContent = workingFor(Number(el.dataset.since));
      }, 1000);
      if (ticking.unref) ticking.unref();
    }

    function thinking(on) {
      if (!on) pose(null);
      for (const f of faces()) {
        f.classList.toggle('think', on);
        if (!on) f.classList.remove('talk');
      }
    }

    function talking() {
      pose(null);
      for (const f of faces()) {
        f.classList.remove('think');
        f.classList.add('talk');
      }
    }

    function pose(tool) {
      const wanted = poseOf(tool);
      for (const f of faces()) {
        for (const name of Object.values(POSES)) f.classList.toggle(name, name === wanted);
      }
    }

    let sleepTimer = 0;
    function awake() {
      clearTimeout(sleepTimer);
      const sleeping = faces().some((f) => f.classList.contains('sleep'));
      for (const f of faces()) f.classList.remove('sleep');
      if (sleeping) mood('wake', 900);
      sleepTimer = setTimeout(() => {
        if (state.open && !state.busy) for (const f of faces()) f.classList.add('sleep');
      }, 60000);
    }

    let lastTap = -1;
    let transformed = 0;
    let streak = 0;
    let lastTapAt = 0;
    function tapped() {
      const now = Date.now();
      if (now < transformed) return;
      streak = now - lastTapAt < STREAK_GAP_MS ? streak + 1 : 1;
      lastTapAt = now;
      const demon = demonMode();
      if (streak >= TICKLED_TOO_LONG || Math.random() < 1 / MONSTER_ONE_IN) {
        streak = 0;
        transformed = now + TRANSFORM_MS;
        mood(demon ? 'sweet' : 'monster', TRANSFORM_MS);
        return;
      }
      const taps = demon ? DEMON_TAPS : TAPS;
      let pick = Math.floor(Math.random() * taps.length);
      if (pick === lastTap) pick = (pick + 1) % taps.length;
      lastTap = pick;
      mood(taps[pick], 900);
    }

    function gaze(e) {
      for (const f of faces()) {
        const box = f.getBoundingClientRect();
        if (!box.width) continue;
        const dx = e.clientX - (box.left + box.width / 2);
        const dy = e.clientY - (box.top + box.height / 2);
        const far = Math.max(1, Math.hypot(dx, dy));
        f.style.setProperty('--gaze-x', `${((dx / far) * Math.min(1, far / 160) * 2).toFixed(2)}px`);
        f.style.setProperty('--gaze-y', `${((dy / far) * Math.min(1, far / 160) * 1.5).toFixed(2)}px`);
      }
    }
    blink();
    document.addEventListener('visibilitychange', blink);
    root.addEventListener('leo-visibility', blink);

    try {
      state.access = accessOf(storage.getItem(ACCESS_KEY));
    } catch (e) {
      state.access = 'ask';
    }

    function drawAccess() {
      const now = ACCESS.find((a) => a.id === state.access);
      const button = $('#chat-access');
      button.className = `chat-access is-${now.id}`;
      button.title = `${now.label}: ${now.hint} Shift+Tab switches.`;
      button.innerHTML = `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${ACCESS_GLYPHS[now.id]}</svg><span>${escape(now.short)}</span><svg class="chev" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M7 10l5 5 5-5"/></svg>`;
      for (const item of panel.querySelectorAll('[data-chat="access-pick"]')) item.setAttribute('aria-checked', String(item.dataset.access === now.id));
    }

    function setAccess(id) {
      state.access = accessOf(id);
      try {
        storage.setItem(ACCESS_KEY, state.access);
      } catch (e) {}
      accessMenu(false);
      drawAccess();
      if (state.busy) { state.busy.permissionWanted = state.access; postPermission(state.busy); }
    }
    drawAccess();

    async function postPermission(work) {
      if (state.busy !== work || !work.answerId || work.permissionWorking || work.permissionPosted === work.permissionWanted) return;
      work.permissionWorking = true;
      const wanted = work.permissionWanted;
      let failed = false;
      const controller = new AbortController();
      const timer = setTimeout(() => controller.abort(), PERMISSION_MS);
      try {
        const reply = await request(`/api/chat/${encodeURIComponent(work.answerId)}/access`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ access: wanted }), signal: controller.signal });
        if (!reply.ok && reply.status !== 410) throw new Error('Permissions could not be changed for the current reply. Send your message again with the access you want.');
        work.permissionPosted = wanted;
      } catch (e) {
        failed = true;
        if (state.busy === work) { work.abort(); notify('Permissions could not be changed for the current reply. Send your message again with the access you want.'); }
      } finally {
        clearTimeout(timer); work.permissionWorking = false;
        if (state.busy === work && work.permissionPosted !== work.permissionWanted && !failed && !controller.signal.aborted) setTimeout(() => postPermission(work), 0);
      }
    }

    const SIDEBAR = 'leo-chat-sidebar';
    const wide = () => Boolean(root.matchMedia && root.matchMedia('(min-width: 900px)').matches);

    const WIDTH = 'leo-chat-width';
    const LEAST_WIDTH = 420;
    const grip = document.createElement('div');
    grip.className = 'chat-resize';
    grip.tabIndex = 0;
    grip.title = 'Drag to make Felix wider or narrower; double-click to reset';
    grip.setAttribute('role', 'separator');
    grip.setAttribute('aria-orientation', 'vertical');
    grip.setAttribute('aria-label', 'Felix width');
    panel.prepend(grip);

    function widest() {
      const beside = parseFloat(root.getComputedStyle(document.body).paddingLeft) || 0;
      const room = root.innerWidth >= 1200 ? 380 : 120;
      return Math.max(LEAST_WIDTH, root.innerWidth - beside - room);
    }

    const chatResize = root.leoPaneResize.create({
      handle: grip,
      key: WIDTH,
      storage,
      docked: 'right',
      measure: () => panel.getBoundingClientRect().width,
      limits: () => [LEAST_WIDTH, widest()],
      apply: (width) => {
        if (width === null) document.documentElement.style.removeProperty('--chat-w');
        else document.documentElement.style.setProperty('--chat-w', `${width}px`);
      },
      enabled: () => wide() && !panel.hidden,
    });

    const HISTORY_OVERLAY_WIDTH = 490;
    const SLIM_WIDTH = 440;
    const sourceNarrow = () => {
      if (panel.hidden) return false;
      const width = panel.getBoundingClientRect().width;
      return (document.body.classList.contains('with-source') && width < HISTORY_SIDE_WIDTH) || width < HISTORY_OVERLAY_WIDTH;
    };
    const HISTORY_LEAST_WIDTH = 190;
    const HISTORY_MOST_WIDTH = 360;
    const HISTORY_CHAT_ROOM = 300;
    const history = panel.querySelector('.chat-history');
    const historyGrip = document.createElement('div');
    historyGrip.className = 'pane-resize chat-history-resize';
    historyGrip.tabIndex = 0;
    historyGrip.title = 'Drag to resize your chats; double-click to reset';
    historyGrip.setAttribute('role', 'separator');
    historyGrip.setAttribute('aria-orientation', 'vertical');
    historyGrip.setAttribute('aria-label', 'Chats list width');
    historyGrip.setAttribute('aria-controls', 'chat-history');
    history.appendChild(historyGrip);
    const historyResize = root.leoPaneResize.create({
      handle: historyGrip,
      key: 'leo-chat-history-width',
      storage,
      measure: () => history.getBoundingClientRect().width,
      limits: () => [HISTORY_LEAST_WIDTH, Math.max(HISTORY_LEAST_WIDTH, Math.min(HISTORY_MOST_WIDTH, panel.clientWidth - HISTORY_CHAT_ROOM))],
      apply: (width) => {
        if (width === null) panel.style.removeProperty('--chat-history-w');
        else panel.style.setProperty('--chat-history-w', `${width}px`);
      },
      enabled: () => !panel.hidden && wide() && !sourceNarrow() && panel.classList.contains('with-history'),
    });
    if (typeof root.ResizeObserver === 'function') {
      const historySize = new root.ResizeObserver(() => {
        document.body.style.setProperty('--chat-over', `${panel.hidden ? 0 : Math.round(panel.getBoundingClientRect().width)}px`);
        const narrow = sourceNarrow();
        const crossed = narrow && !panel.classList.contains('source-narrow') && panel.classList.contains('with-history');
        const swap = !panel.hidden && narrow !== panel.classList.contains('source-narrow');
        if (swap) panel.classList.add('mode-switch');
        if (!panel.hidden) panel.classList.toggle('source-narrow', narrow);
        if (crossed) {
          state.sidebar = null;
          syncSidebar(false);
        }
        if (state.sidebar === null && !panel.hidden) {
          const on = sidebarOpen();
          const before = panel.classList.contains('with-history');
          syncSidebar(on);
          if (on && !before && !state.chats) loadChats();
        }
        syncSidebar(panel.classList.contains('with-history'));
        if (swap) {
          void panel.offsetWidth;
          panel.classList.remove('mode-switch');
        }
        historyResize.fit();
      });
      historySize.observe(panel);
      const chatMain = panel.querySelector('.chat-main');
      const syncSlim = () => {
        if (panel.hidden || (history.getAnimations && history.getAnimations().length)) return;
        const width = chatMain.getBoundingClientRect().width;
        if (!width || (width < SLIM_WIDTH) === panel.classList.contains('slim')) return;
        panel.classList.toggle('slim', width < SLIM_WIDTH);
      };
      new root.ResizeObserver(syncSlim).observe(chatMain, { box: 'border-box' });
      const settled = (e) => {
        if (e.target === history) syncSlim();
      };
      history.addEventListener('transitionend', settled);
      history.addEventListener('transitioncancel', settled);
    }

    function sidebarOpen() {
      if (state.sidebar !== null) return state.sidebar;
      try {
        const kept = storage.getItem(SIDEBAR);
        if (kept !== null) return kept === '1' && wide() && !sourceNarrow();
      } catch (e) {
        return false;
      }
      return false;
    }

    function syncSidebar(on) {
      const overlay = !wide() || sourceNarrow();
      const button = panel.querySelector('.chat-history-toggle');
      const main = panel.querySelector('.chat-main');
      const back = !on && history.contains(document.activeElement);
      const enter = on && overlay && main.contains(document.activeElement);
      main.inert = on && overlay;
      history.inert = !on;
      history.setAttribute('aria-hidden', String(!on));
      button.setAttribute('aria-expanded', String(on));
      panel.classList.toggle('with-history', on);
      if (back) button.focus({ preventScroll: true });
      else if (enter) history.querySelector('.chat-history-back').focus({ preventScroll: true });
    }

    function showSidebar(on) {
      state.sidebar = on;
      if (on) root.leoMotion?.cascade($('#chat-history-list'));
      if (wide() && !sourceNarrow()) {
        try {
          storage.setItem(SIDEBAR, on ? '1' : '0');
        } catch (e) {
          state.sidebar = on;
        }
      }
      syncSidebar(on);
      historyResize.fit();
      if (on && !state.chats) loadChats();
    }

    function leaveOverlay() {
      state.sidebar = null;
      syncSidebar(false);
    }

    function closeHistory() {
      if (!wide() || sourceNarrow()) {
        leaveOverlay();
        state.sidebar = false;
      } else showSidebar(false);
    }

    syncSidebar(false);

    function stored() {
      return state.messages.filter((m) => !m.pending);
    }

    function persist() {
      save(storage, stored(), state.refs, state.id);
    }

    // Idle compaction: after warm.rs IDLE the warm session and the provider's
    // prompt cache are cold, so the next message would resend every recent turn
    // at full price. leo summarises the older part with the cheap model instead
    // (POST /api/chat/compact); the chat on screen stays exactly as it is.
    let compactTimer = 0;
    function stopCompacting() {
      clearTimeout(compactTimer);
    }
    function compactLater() {
      clearTimeout(compactTimer);
      const id = state.id;
      if (id) compactTimer = setTimeout(() => compactNow(id), COMPACT_IDLE_MS);
    }
    async function compactNow(id) {
      if (state.busy || state.id !== id) return;
      const messages = modelHistory(state.messages.filter((m) => !m.pending));
      if (messages.length < COMPACT_LEAST_TURNS) return;
      try {
        await request('/api/chat/compact', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ chat: id, messages }) });
      } catch (e) {}
    }

    let upload = Promise.resolve();
    function snapshot() {
      return { id: state.id, refs: state.refs, messages: stored() };
    }

    function remember(chat = snapshot()) {
      if (chat.id === state.id) persist();
      const messages = chat.messages.filter((m) => !m.pending);
      if (!messages.length) return upload;
      const id = chat.id;
      const sending = { refs: chat.refs, messages: messages.slice(-MOST_SAVED).map(({ pending, ...m }) => m) };
      upload = upload.then(async () => {
        try {
          const response = await request(`/api/chats/${encodeURIComponent(id)}`, {
            method: 'PUT',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify(sending),
          }).catch(() => null);
          if (!response || !response.ok) {
            if (!unsaved.has(id)) {
              unsaved.add(id);
              notify('Felix could not save this chat, so it may be missing from your history.');
            }
            return;
          }
          unsaved.delete(id);
          const summary = await response.json();
          if (state.chats) {
            state.chats = [summary, ...state.chats.filter((c) => c.id !== summary.id)].sort((a, b) => (Date.parse(b.updated_at) || 0) - (Date.parse(a.updated_at) || 0));
            drawChats();
          }
          if (!summary.named && summary.count >= 2 && !naming.has(summary.id)) {
            naming.add(summary.id);
            for (const wait of NAME_CHECKS) setTimeout(() => state.chats && loadChats(), wait);
          }
        } catch (e) {
          return;
        }
      });
      return upload;
    }

    const naming = new Set();
    const unsaved = new Set();
    const NAME_CHECKS = [8000, 25000, 60000];

    async function loadChats() {
      try {
        const response = await request('/api/chats');
        if (response.ok) state.chats = await response.json();
      } catch (e) {
        state.chats = state.chats || [];
      }
      drawChats();
    }

    let findTimer = 0;
    let findTurn = 0;
    async function findChats(query) {
      const turn = ++findTurn;
      if (!query.trim()) {
        state.found = null;
        drawChats();
        return;
      }
      try {
        const response = await request(`/api/chats?q=${encodeURIComponent(query.trim())}`);
        if (!response.ok || turn !== findTurn) return;
        state.found = await response.json();
      } catch (e) {
        state.found = [];
      }
      if (turn === findTurn) drawChats();
    }

    function drawChats() {
      drawTitle();
      const box = $('#chat-history-list');
      if (!box) return;
      if (!state.chats) {
        box.innerHTML = '<div class="chat-history-none">Loading…</div>';
        return;
      }
      if (state.found) {
        box.innerHTML = state.found.length
          ? state.found.map((c) => chatRow(c)).join('')
          : '<div class="chat-history-none">No chat mentions that.</div>';
        return;
      }
      const first = state.messages.find((m) => m.role === 'user' && m.text);
      const ghost = state.fresh && !state.chats.some((c) => c.id === state.id)
        ? [{ id: state.id, title: first ? draftTitle(first) : 'New chat', updated_at: new Date().toISOString(), ghost: true }]
        : [];
      const list = [...ghost, ...state.chats];
      if (!list.length) {
        box.innerHTML = `<div class="chat-history-none">${drawings.chats || ''}Your conversations with Felix are kept here, on this computer.</div>`;
        return;
      }
      box.innerHTML = groups(list)
        .map((g) => `<div class="chat-history-group">${escape(g.name)}</div>${g.chats
          .map((c) => chatRow(c))
          .join('')}`)
        .join('');
    }

    function chatRow(c) {
      return `<div class="chat-history-row${c.id === state.id ? ' on' : ''}"><button class="chat-history-item" data-chat="resume" data-id="${escape(c.id)}" title="${escape(c.title)}"><span class="chat-history-title">${escape(c.title)}</span></button>${c.ghost ? '' : `<button class="chat-history-x${state.doomed === c.id ? ' sure' : ''}" data-chat="forget" data-id="${escape(c.id)}" aria-label="${state.doomed === c.id ? 'Confirm delete' : 'Delete'} ${escape(c.title)}"><span class="chat-delete-x" aria-hidden="true">×</span><span class="chat-delete-label" aria-hidden="true">Delete</span></button>`}</div>`;
    }

    let resumeSeq = 0;

    function begin(id = newId()) {
      resumeSeq++;
      if (state.busy) state.busy.abort();
      state.id = id;
      state.fresh = true;
      stopCompacting();
      state.messages = [];
      state.refs = [];
      state.files = [];
      state.sent = [];
      state.filesFor = state.id;
      state.streak = 0;
      state.asking = null;
      closePick();
      drawRefs();
      persist();
      draw();
      drawChats();
      loadReview();
      mood('wave', 1500);
    }

    async function resume(id) {
      if (id === state.id) {
        if (!wide() || sourceNarrow()) leaveOverlay();
        return true;
      }
      if (state.busy) state.busy.abort();
      const mine = ++resumeSeq;
      let chat;
      try {
        const response = await request(`/api/chats/${encodeURIComponent(id)}`);
        if (mine !== resumeSeq) return null;
        if (response.status === 404) {
          state.chats = (state.chats || []).filter((c) => c.id !== id);
          drawChats();
          return false;
        }
        if (!response.ok) throw new Error('Leo could not open that conversation. Try again.');
        chat = await response.json();
        if (mine !== resumeSeq) return null;
      } catch (e) {
        if (mine === resumeSeq) notify('Leo could not open that conversation. Check your connection and try again.');
        return null;
      }
      state.id = chat.id;
      state.fresh = false;
      compactLater();
      state.messages = Array.isArray(chat.messages) ? chat.messages : [];
      state.refs = Array.isArray(chat.refs) ? chat.refs.slice(0, MOST_REFS) : [];
      state.files = [];
      state.sent = [];
      state.streak = 0;
      closePick();
      persist();
      await loadFiles();
      if (mine !== resumeSeq || state.id !== id) return null;
      drawContext();
      drawRefs();
      draw();
      drawChats();
      if (!wide() || sourceNarrow()) leaveOverlay();
      return true;
    }

    function syncChatDelete() {
      for (const button of panel.querySelectorAll('.chat-history-x')) {
        const sure = state.doomed === button.dataset.id;
        button.classList.toggle('sure', sure);
        const title = (button.getAttribute('aria-label') || '').replace(/^(?:Confirm delete|Delete) /, '');
        button.setAttribute('aria-label', `${sure ? 'Confirm delete' : 'Delete'} ${title}`);
      }
    }

    const deleting = new Set();
    async function forget(id) {
      const button = [...panel.querySelectorAll('.chat-history-x')].find((item) => item.dataset.id === id);
      if (state.doomed !== id) {
        state.doomed = id;
        syncChatDelete();
        button?.focus({ preventScroll: true });
        return;
      }
      if (button?.disabled || deleting.has(id)) return;
      const focused = document.activeElement === button;
      if (button) button.disabled = true;
      deleting.add(id);
      try {
        const response = await request(`/api/chats/${encodeURIComponent(id)}`, { method: 'DELETE' });
        if (!response.ok) throw new Error('delete failed');
      } catch (e) {
        notify('Leo could not delete that conversation. Try again.');
        return;
      } finally {
        deleting.delete(id);
        if (button?.isConnected) {
          button.disabled = false;
          if (focused && document.activeElement === document.body) button.focus({ preventScroll: true });
        }
      }
      state.doomed = null;
      state.chats = (state.chats || []).filter((c) => c.id !== id);
      if (id === state.id) begin();
      else drawChats();
    }

    function spare(e) {
      if (!state.doomed || e.target.closest?.('.chat-history-x.sure')) return;
      state.doomed = null;
      syncChatDelete();
    }
    document.addEventListener('keydown', (e) => {
      if (e.key !== 'Escape' || !state.doomed) return;
      e.preventDefault();
      e.stopImmediatePropagation();
      state.doomed = null;
      syncChatDelete();
    }, true);
    document.addEventListener('pointerdown', spare, true);
    document.addEventListener('focusin', spare, true);

    const excerpts = new Map();
    function excerptOf(r) {
      if (excerpts.has(r.id)) return excerpts.get(r.id);
      excerpts.set(r.id, '');
      request(`/api/notes/${encodeURIComponent(r.id)}`)
        .then((response) => (response.ok ? response.json() : null))
        .then((note) => {
          if (!note || typeof note.body !== 'string') return;
          excerpts.set(r.id, noteExcerpt(note.title || r.title, note.body));
          drawRefs();
          if (state.messages.some((m) => m.role === 'user' && Array.isArray(m.refs) && m.refs.some((x) => x.id === r.id))) draw(false);
        })
        .catch(() => {});
      return '';
    }

    function drawRefs() {
      const p = state.selection;
      const passage = p
        ? `<span class="chat-ref chat-passage"><button class="chat-ref-open" data-chat="open" data-id="${escape(p.note)}" data-line="${p.from}-${p.to}" title="${escape(clipped(p.text, 400))}"><svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M7 7h4v4c0 2.5-1.5 4-4 5M14 7h4v4c0 2.5-1.5 4-4 5"/></svg>${escape(p.title || 'Note')}<span class="chat-passage-text">${escape(clipped(p.text.replace(/\s+/g, ' '), 60))}</span></button><button class="chat-ref-x" data-chat="unselect" aria-label="Remove the selected passage">×</button></span>`
        : '';
      const notes = passage;
      const files = state.refs.map((r) => noteCard(r, escape, { excerpt: excerptOf(r), remove: true })).join('') + state.files
        .map((f) => (f.status === 'reading' ? fileCard(f, escape, { reading: true }) : fileCard(f, escape, { remove: f.id })))
        .join('');
      $('#chat-refs').innerHTML = (notes ? `<div class="chat-notes">${notes}</div>` : '') + (files ? `<div class="file-cards">${files}</div>` : '');
    }

    async function loadFiles() {
      const chat = state.id;
      state.filesFor = chat;
      try {
        const response = await request(`/api/chats/${encodeURIComponent(chat)}/files`);
        if (!response.ok || state.id !== chat) return;
        const split = splitFiles(await response.json(), state.messages);
        const known = new Set([...split.waiting, ...split.sent].map((d) => d.id));
        const added = state.files.filter((f) => f.status === 'reading' || (f.id && !known.has(f.id)));
        state.sent = split.sent;
        state.files = [...split.waiting.map((d) => ({ ...d, status: 'ready' })), ...added];
        drawRefs();
      } catch (e) {
        state.files = state.id === chat ? state.files : [];
      }
    }

    async function thumbOf(file) {
      if (!/^image\//.test(file.type) || typeof createImageBitmap !== 'function') return null;
      try {
        const bitmap = await createImageBitmap(file);
        const scale = Math.min(1, 240 / Math.max(bitmap.width, bitmap.height));
        const canvas = document.createElement('canvas');
        canvas.width = Math.max(1, Math.round(bitmap.width * scale));
        canvas.height = Math.max(1, Math.round(bitmap.height * scale));
        canvas.getContext('2d').drawImage(bitmap, 0, 0, canvas.width, canvas.height);
        const url = canvas.toDataURL('image/jpeg', 0.72);
        return THUMB.test(url) ? url : null;
      } catch (e) {
        return null;
      }
    }

    const reading = new Map();

    function addFile(file) {
      const key = `${Date.now()}-${Math.random()}`;
      state.files.push({ key, name: file.name, status: 'reading', thumb: null });
      drawRefs();
      const work = readFile(file, key, state.id);
      reading.set(key, work);
      work.then(() => reading.delete(key));
      return work;
    }

    async function readFile(file, key, chat) {
      const thumb = await thumbOf(file);
      if (thumb) {
        state.files = state.files.map((f) => (f.key === key ? { ...f, thumb } : f));
        drawRefs();
      }
      const drop = () => {
        state.files = state.files.filter((f) => f.key !== key);
        drawRefs();
        return null;
      };
      let reply;
      try {
        const body = await prepare(file);
        reply = await request(`/api/chats/${encodeURIComponent(chat)}/files`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify(body),
        });
      } catch (e) {
        notify(e.message || `Felix could not read ${file.name}.`);
        return drop();
      }
      if (!reply.ok) {
        let said = '';
        try {
          said = (await reply.json()).error || '';
        } catch (e) {
          said = '';
        }
        notify(said ? `Felix could not read ${file.name}: ${said}` : reply.status === 413 ? `${file.name} is too large for Felix.` : `Felix could not read ${file.name}.`);
        return drop();
      }
      const doc = { ...(await reply.json()), key, status: 'ready', thumb };
      if (state.id === chat) {
        state.files = state.files.map((f) => (f.key === key ? doc : f));
        drawRefs();
      }
      return doc;
    }

    function cardOf(f) {
      const card = { name: f.name };
      if (f.thumb) card.thumb = f.thumb;
      else if (f.excerpt) card.excerpt = String(f.excerpt).slice(0, 320);
      if (f.status === 'reading') card.reading = true;
      return card;
    }

    function addFiles(list) {
      const room = MOST_FILES - state.files.length - state.sent.length;
      const files = [...list];
      if (files.length > room) notify(`A chat holds up to ${MOST_FILES} documents, so ${files.length - Math.max(room, 0)} ${files.length - Math.max(room, 0) === 1 ? 'was' : 'were'} left out.`);
      for (const file of files.slice(0, Math.max(room, 0))) addFile(file);
    }

    async function removeFile(id) {
      const chat = state.id;
      const at = state.files.findIndex((f) => f.id === id);
      const file = state.files[at];
      state.files = state.files.filter((f) => f.id !== id);
      drawRefs();
      let removed = false;
      try {
        const response = await request(`/api/chats/${encodeURIComponent(chat)}/files/${encodeURIComponent(id)}`, { method: 'DELETE' });
        removed = response.ok || response.status === 404 || response.status === 401;
      } catch (e) {
        removed = false;
      }
      if (removed) return;
      if (file && state.id === chat && !state.files.some((f) => f.id === id)) {
        state.files = [...state.files.slice(0, at), file, ...state.files.slice(at)];
        drawRefs();
      }
      notify(`Leo could not remove ${file?.name || 'that document'}. Try again.`);
    }

    let pickTimer = 0;
    let pickSeq = 0;
    async function findNotes(query) {
      const url = query.trim() ? `/api/search?q=${encodeURIComponent(query.trim())}` : '/api/notes?limit=12';
      const response = await request(url);
      if (!response.ok) return [];
      const notes = await response.json();
      return notes.filter((n) => !state.refs.some((r) => r.id === n.id)).slice(0, 8);
    }

    function closePick() {
      state.pick = null;
      const box = $('#chat-pick');
      popMenu(box, false, input, () => { if (!state.pick) box.innerHTML = ''; });
    }

    function drawPick() {
      const box = $('#chat-pick');
      const p = state.pick;
      if (!p) return closePick();
      if (box.hidden || box.dataset.folding) popMenu(box, true, input);
      const rows = p.notes.length
        ? p.notes.map((n, i) => `<button type="button" class="chat-pick-row${i === p.at ? ' on' : ''}" data-chat="pick" data-i="${i}" role="option" aria-selected="${i === p.at}"><b>${escape(n.title || 'Untitled')}</b><span>${escape(n.directory || 'All notes')}</span></button>`).join('')
        : `<div class="chat-pick-none">${p.loading ? 'Looking…' : state.refs.length >= MOST_REFS ? `Up to ${MOST_REFS} notes at once.` : 'No notes match.'}</div>`;
      const wantsSearch = p.from === 'button';
      let field = $('#chat-pick-search');
      let list = box.querySelector('.chat-pick-list');
      if (!list || Boolean(field) !== wantsSearch) {
        box.innerHTML = `${wantsSearch ? `<input class="chat-pick-search" id="chat-pick-search" placeholder="Find a note…" autocomplete="off" value="${escape(p.query)}">` : ''}<div class="chat-pick-list" role="listbox" aria-label="Notes"></div>`;
        field = $('#chat-pick-search');
        list = box.querySelector('.chat-pick-list');
        list.addEventListener('wheel', (event) => {
          if (state.pick?.from === 'mention') containChatPickWheel(list, event);
        }, { passive: false });
      }
      list.innerHTML = rows;
      revealChatPick(list);
      if (field && p.focusSearch) {
        field.focus();
        field.setSelectionRange(field.value.length, field.value.length);
        p.focusSearch = false;
      }
    }

    function openPick(from, query, start) {
      if (state.refs.length >= MOST_REFS) {
        state.pick = { from, query, start, notes: [], at: 0, loading: false };
        return drawPick();
      }
      const same = state.pick && state.pick.from === from && state.pick.query === query;
      if (same) return;
      state.pick = { from, query, start, notes: state.pick ? state.pick.notes : [], at: 0, loading: true, focusSearch: from === 'button' && !state.pick };
      drawPick();
      clearTimeout(pickTimer);
      const mine = ++pickSeq;
      pickTimer = setTimeout(async () => {
        let notes = [];
        try {
          notes = await findNotes(query);
        } catch (e) {
          notes = [];
        }
        if (mine !== pickSeq || !state.pick) return;
        state.pick.notes = notes;
        state.pick.loading = false;
        state.pick.at = 0;
        drawPick();
      }, 120);
    }

    function choose(i) {
      const p = state.pick;
      const note = p && p.notes[i];
      if (!note) return;
      state.refs = addRef(state.refs, note);
      if (p.from === 'mention') {
        const caret = input.selectionStart;
        input.value = input.value.slice(0, p.start) + input.value.slice(caret);
        input.setSelectionRange(p.start, p.start);
        fit();
      }
      closePick();
      drawRefs();
      remember();
      if (state.asking && state.asking.starter.needs === 'note') {
        const starter = state.asking.starter;
        state.asking = null;
        send(starter.text);
        return;
      }
      if (!state.messages.length) draw();
      input.focus();
    }

    function watchMention() {
      const m = mentionAt(input.value, input.selectionStart);
      if (m) openPick('mention', m.query, m.start);
      else if (state.pick && state.pick.from === 'mention') closePick();
    }

    function currentSession() {
      for (let k = state.messages.length - 1; k >= 0; k--) {
        const s = state.messages[k].session;
        if (s && typeof s === 'object' && Array.isArray(s.ideas) && Array.isArray(s.outcomes)) return s.dismissed ? null : s;
      }
      return null;
    }

    function drawProgress() {
      const box = $('#study-progress');
      const s = currentSession();
      if (!s || (s.state !== 'active' && s.state !== 'finishing')) {
        box.hidden = true;
        box.innerHTML = '';
        return;
      }
      const c = sessionSummary(s);
      const latest = state.messages.slice(state.messages.findLastIndex((m) => m.session === s)).flatMap((m) => m.quizzes || []).findLast((q) => q.idea);
      const asking = latest && typeof latest.given !== 'string' ? latest.idea : 0;
      const ideas = s.ideas.map((t, i) => {
        const o = s.outcomes[i];
        const now = o.state === 'open' && asking === i + 1;
        const once = o.state === 'open' && o.right > 0;
        const mark = o.state === 'first' || o.state === 'done' ? `<path class="study-ok" d="${STUDY_MARKS.ok}"/>` : once ? `<path class="study-ok study-once" d="${STUDY_MARKS.ok}"/>` : o.state === 'review' ? `<path class="study-no" d="${STUDY_MARKS.no}"/>` : '';
        const said = { first: 'right twice, never missed', done: 'right twice', review: 'to look at again' }[o.state] || (once ? (now ? 'right once, asking again' : 'right once, one more to go') : now ? 'asking now' : 'not yet');
        return `<span class="study-idea ${o.state}${now ? ' now' : ''}" title="${escape(t)}"><svg class="study-mark" viewBox="0 0 18 18" role="img" aria-label="${said}"><path class="study-ring" d="${STUDY_MARKS.rings[i % STUDY_MARKS.rings.length]}"/>${mark}</svg>${escape(t)}</span>`;
      }).join('');
      box.hidden = false;
      box.innerHTML = `<div class="study-ideas" aria-label="Ideas in this session">${ideas}</div><span class="study-count">${c.covered} of ${c.total} ${c.total === 1 ? 'idea' : 'ideas'}</span><button type="button" class="msg-act" data-chat="session-finish">Finish</button>`;
    }

    function recapCard(s) {
      const c = sessionSummary(s);
      const parts = [`${c.covered} of ${c.total} ${c.total === 1 ? 'idea' : 'ideas'} covered`];
      if (c.first) parts.push(`${c.first} without a miss`);
      const lines = [];
      if (c.review.length) lines.push(`<p><b>Look at again:</b> ${c.review.map(escape).join(', ')}</p>`);
      if (c.once.length) lines.push(`<p><b>Right once:</b> ${c.once.map(escape).join(', ')}</p>`);
      if (c.skipped.length) lines.push(`<p><b>Not shown yet:</b> ${c.skipped.map(escape).join(', ')}</p>`);
      const again = [...c.review, ...c.once, ...c.skipped].length;
      return `<div class="session-recap" role="status"><h4>Study session done</h4><p>${parts.join(' · ')}</p>${lines.join('')}<div class="session-acts">${again ? '<button type="button" class="btn primary sm" data-chat="session-again">Study those again</button>' : ''}<button type="button" class="btn plain sm" data-chat="session-done">Done</button></div></div>`;
    }

    const EYE = '<path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7-10-7-10-7z"/><circle cx="12" cy="12" r="3"/>';
    const EYE_OFF = '<path d="M3 3l18 18"/><path d="M10.6 5.1A10 10 0 0 1 12 5c6.5 0 10 7 10 7a18 18 0 0 1-3.2 4.2M6.6 6.6A17 17 0 0 0 2 12s3.5 7 10 7a10 10 0 0 0 5.4-1.6"/><path d="M9.9 9.9a3 3 0 0 0 4.2 4.2"/>';

    function drawContext() {
      const ctx = state.context && state.dropped !== state.context.id ? state.context : null;
      const chip = $('[data-chat="drop"]');
      const off = Boolean(state.context) && !ctx;
      const label = off ? 'Read this note again' : 'Stop reading this note';
      chip.hidden = !state.context;
      chip.classList.toggle('off', off);
      chip.title = label;
      chip.setAttribute('aria-label', label);
      chip.firstElementChild.innerHTML = off ? EYE_OFF : EYE;
      chip.lastElementChild.textContent = state.context ? state.context.title || 'Untitled' : '';
    }

    const clippedTitle = (text) => {
      const chars = Array.from(String(text).replace(/\s+/g, ' ').trim());
      return chars.length > 80 ? `${chars.slice(0, 80).join('')}…` : chars.join('');
    };

    function draftTitle(first) {
      const asked = clippedTitle(first.text);
      const open = state.context && state.dropped !== state.context.id ? state.context.title : undefined;
      const note = [first.refs?.[0]?.title, first.docs?.[0], state.refs[0]?.title, open].find((t) => typeof t === 'string');
      if (!note || !note.trim() || asked.split(' ').length > 6 || asked.toLowerCase().includes(note.toLowerCase())) return asked;
      return clippedTitle(`${asked.replace(/[?.! ]+$/, '')}: ${note}`);
    }

    function drawTitle() {
      const first = state.messages.find((m) => m.role === 'user' && m.text);
      const saved = (state.chats || []).find((c) => c.id === state.id);
      const title = $('#chat-title');
      title.textContent = saved ? saved.title : first ? draftTitle(first) : 'New chat';
      title.classList.toggle('quiet', !saved && !first);
    }

    const writer = (m) => (m && m.spent && typeof m.spent.by === 'string' ? [m.spent.by, m.spent.model].filter(Boolean).join(' · ') : '');

    function switches(list, i) {
      const m = list[i];
      const out = [];
      if (m.role === 'assistant' && writer(m)) {
        const before = list.slice(0, i).reverse().find((x) => x.role === 'assistant' && writer(x) && !x.spent.auto);
        if (before && writer(before) !== writer(m) && !m.spent.auto) out.push(`Now answered by ${writer(m)}, with everything said so far`);
      }
      return out.map((t) => `<div class="chat-switch">${escape(t)}</div>`).join('');
    }

    function bubble(m, i) {
      if (m.role === 'user' && m.next) return '<div class="chat-switch msg-next" role="separator">Next question</div>';
      if (m.role === 'user') {
        const notes = (m.refs || []).filter((r) => r && r.id).map((r) => noteCard(r, escape, { excerpt: excerptOf(r) }));
        const cards = Array.isArray(m.cards)
          ? m.cards.filter((c) => c && typeof c.name === 'string')
          : [...(m.docs || []).map((name) => ({ name })), ...(m.pics || []).filter((p) => p && typeof p.name === 'string')];
        const p = m.selection && typeof m.selection.text === 'string' ? m.selection : null;
        const quote = p
          ? `<div class="msg-passage"><button class="msg-passage-from" data-chat="open" data-id="${escape(p.note || '')}" data-line="${Number(p.from) || 1}-${Number(p.to) || Number(p.from) || 1}">${escape(p.title || 'Note')}${p.from ? ` · ${p.to > p.from ? `lines ${p.from}–${p.to}` : `line ${p.from}`}` : ''}</button><div class="msg-passage-text">${escape(clipped(p.text, 280))}</div></div>`
          : '';
        const files = notes.length || cards.length ? `<div class="file-cards sent">${notes.join('')}${cards.filter(Boolean).map((c) => fileCard(c, escape, { reading: c.reading === true })).join('')}</div>` : '';
        const quiz = m.quiz && typeof m.quiz === 'object'
          ? `<div class="msg-note">${m.quiz.correct === true ? 'Practice answer · correct' : m.quiz.correct === false ? 'Practice answer · not quite' : 'Practice answer'}</div>`
          : '';
        const steer = m.steer === 'waiting' ? '<div class="msg-note">Felix reads this at his next step</div>' : m.steer === 'read' ? '<div class="msg-note">Sent while Felix worked</div>' : '';
        const edit = i === lastPair() ? `<div class="msg-acts"><button class="msg-act" data-chat="edit"${state.busy ? ' disabled' : ''}>Edit</button></div>` : '';
        return `<div class="msg user${m.steer === 'waiting' ? ' queued' : ''}">${quote}${files}<div class="bubble">${escape(m.text).replace(/\n/g, '<br>')}</div>${quiz}${steer}${edit}</div>`;
      }
      const spoken = grade(m.text).text;
      const shown = m.pending ? held(spoken) : spoken;
      const html = !shown && m.pending ? '<span class="typing"><i></i><i></i><i></i></span>' : '';
      const used = cited(m.text, m.sources);
      const marks = numbered(shown, m.sources);
      const from = !m.pending && marks.length
        ? `<div class="msg-from">${marks.map((x) => `<button class="cite cite-src" data-chat="open" data-id="${escape(x.s.id)}"${x.lines ? ` data-line="${x.lines}"` : ''}><b>${x.n}</b>${escape(x.s.title)}${x.where ? ` · ${escape(x.where)}` : ''}</button>`).join('')}</div>`
        : '';
      const error = noAi(m.error)
        ? `${drawings.noink || ''}<div class="prose"><p>${escape(m.error)}</p></div><div class="msg-acts"><a class="btn primary sm" href="#/settings/writing">Set up writing AI</a></div>`
        : m.error ? `<div class="msg-error">${escape(m.error)}</div>` : '';
      const verdict = grade(m.text).verdict;
      const badge = verdict ? `<span class="verdict ${verdict}">${verdict === 'correct' ? 'Correct' : 'Not quite'}</span>` : '';
      const label = m.pending ? null : spentLabel(m.spent);
      const meta = label ? `<span class="msg-meta" title="${escape(label.title)}">${escape(label.text)}</span>` : '';
      const save = !m.pending && !m.error && shown
        ? (m.saved
          ? `<button class="msg-act" data-chat="open" data-id="${escape(m.saved)}">Open the saved note</button>`
          : `<button class="msg-act" data-chat="save" data-i="${i}">Save as note</button>`) + `<button class="msg-act" data-chat="add" data-i="${i}">Add to note</button>`
          + `<button class="msg-act" data-chat="copy" data-i="${i}"${state.busy ? ' disabled' : ''}>Copy</button>`
          + (i === lastPair() + 1 ? `<button class="msg-act" data-chat="retry"${state.busy ? ' disabled' : ''}>Retry</button>` : '')
        : '';
      const drew = !m.pending && !m.error && /^\s*(```|~~~)\s*(mermaid|svg)\b/im.test(shown);
      const bigger = drew ? '<button type="button" class="msg-act icon-act" data-action="diagram-expand" data-reply="all" aria-label="See it larger" title="See it larger"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M14 4h6v6M10 20H4v-6M20 4l-7 7M4 20l7-7"/></svg></button>' : '';
      const larger = drew ? '<button type="button" class="see-larger" data-action="diagram-expand" data-reply="here">See it larger</button>' : '';
      const keep = save || meta || bigger ? `<div class="msg-acts">${save}${meta}${bigger}</div>` : '';
      const removed = verdict ? m.text.length - spoken.length : 0;
      const parts = timeline(shown, (Array.isArray(m.steps) ? m.steps : []).slice(0, MOST_REPLY_STEPS).map((s) => typeof s === 'string' || !s ? s : { ...s, at: Number.isInteger(s.at) ? s.at - removed : 0 }));
      const taken = fixedSteps(parts.flatMap((part) => part.steps || []), !m.pending);
      const made = (m.proposals || []).filter((p) => p && p.state === 'applied').length;
      const prose = (text) => `<div class="prose">${cite(render(text, { rows: true }), m.sources, escape, m.files, marks).replace(/<input /g, '<input disabled ')}</div>`;
      const now = m.pending && taken.length ? stepRuns(taken.some((s) => s && !s.failed) ? taken.filter((s) => s && !s.failed) : taken).at(-1) : null;
      const trace = now
        ? `<div class="msg-status" role="status">${stepLine({ ...now.step, found: [] }, true, now.times)}</div>`
        : !m.pending && taken.length
        ? `<details class="msg-trace" data-i="${i}"${openTraces.has(m) ? ' open' : ''}><summary>${m.error ? '' : '<span class="pencil-tick" aria-hidden="true"></span>'}${escape(traceSummary(taken, used.map((s) => s.title), made))}${timed.has(m) && Math.round(m.elapsed_ms / 1000) >= 1 ? `<span class="msg-trace-took"> · ${tookFor(m.elapsed_ms)}</span>` : ''}${stepChevron}</summary><div class="msg-steps">${stepRuns(taken).map((run) => stepLine(run.step, false, run.times)).join('')}</div></details>`
        : '';
      const words = parts.length ? parts.map((part) => part.steps ? '' : prose(part.text)).join('') : html ? `<div class="prose">${html}</div>` : '';
      const outside = !m.pending && notFromNotes(taken, shown, used.length) ? '<div class="msg-note msg-outside">Not from your notes</div>' : '';
      const body = `${trace}${words}${larger}${outside}`;
      const offers = (m.proposals || []).map((p, j) => proposalCard(p, i, j)).join('');
      const asks = (m.asks || []).map((a, k) => askCard(a, i, k, m.pending)).join('');
      const quizzes = (m.quizzes || []).map((q, k) => quizCard(q, i, k, m.pending, m.sources)).join('');
      const since = m.pending ? working.get(m) : 0;
      if (since) tick();
      const clock = since ? `<div class="msg-note msg-clock" data-since="${since}" role="timer">${workingFor(since)}</div>` : '';
      return `<div class="msg leo${m.pending ? ' pending' : ''}" data-i="${i}">${badge}${body}${quizzes}${asks}${offers}${clock}${error}${from}${keep}</div>`;
    }

    function onNote() {
      return Boolean(state.context && state.dropped !== state.context.id);
    }

    function asking() {
      const a = state.asking;
      if (!a) return '';
      const body = a.starter.needs === 'note'
        ? '<p class="chat-ask-how">Pick one below, or type @ and its name.</p>'
        : a.folders === null
          ? '<p class="chat-ask-how">Looking at your folders…</p>'
          : a.folders.length
            ? `<div class="chat-ask-classes">${a.folders.map((f) => `<button class="chat-class${a.picked.includes(f) ? ' on' : ''}" data-chat="class" data-name="${escape(f)}" aria-pressed="${a.picked.includes(f)}">${escape(f)}</button>`).join('')}</div>
              <div class="chat-ask-go"><button class="btn sm plain" data-chat="classes-all">All of them</button><button class="btn sm primary" data-chat="classes-go"${a.picked.length ? '' : ' disabled'}>Go</button></div>`
            : '<p class="chat-ask-how">You have no folders yet, so I will use all your notes.</p><div class="chat-ask-go"><button class="btn sm primary" data-chat="classes-all">Go</button></div>';
      return `<div class="chat-ask"><div class="chat-ask-top">${felix(34, 'idle')}<b>${escape(a.starter.ask)}</b><button class="chat-ask-x" data-chat="ask-cancel" aria-label="Never mind"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><path d="M6 6l12 12M18 6L6 18"/></svg></button></div>${body}</div>`;
    }

    function welcome() {
      const where = onNote() ? 'this note' : 'your notes';
      return `<div class="chat-hello" data-hello="${escape(`${state.id}:${where}`)}">
        <div class="chat-hello-lead"><b>${greeting("Hi, I'm Felix.", escape)}</b><p class="chat-hello-text">${greeting(`${onNote() ? `I'm reading “${state.context.title || 'Untitled'}” and the notes connected to it.` : "I'm looking through your notes."} Ask anything, or tell me what you want to learn.`, escape)}</p></div>
        ${reviewCard()}
      </div>`;
    }

    function drawBelow() {
      const next = document.createElement('div');
      if (!state.messages.length) {
        const unread = Boolean(state.context) && state.dropped === state.context.id;
        next.innerHTML = `<div class="chat-starters">${STARTERS.map((s, i) => unread && s.needs === 'note' ? '' : `<button class="starter${state.asking && state.asking.starter === s ? ' on' : ''}" data-chat="starter" data-i="${i}">${escape(s.text)}</button>`).join('')}</div>${asking()}`;
      }
      settle($('#chat-below'), next);
    }

    function reviewCard() {
      const n = (state.review || []).filter((m) => m.due).length;
      if (!n) return '';
      const what = `${n} question${n === 1 ? '' : 's'}`;
      return `<div class="chat-review">
        <b>Time to review ${what} you missed</b>
        <span>From your earlier chats. Felix asks ${n === 1 ? 'it' : 'them'} again, one at a time.</span>
        <button class="btn primary" data-chat="review">Review now</button>
      </div>`;
    }

    async function loadReview() {
      try {
        const response = await request('/api/review');
        if (!response.ok) return;
        state.review = await response.json();
      } catch (e) {
        return;
      }
      if (state.open && !state.messages.length) draw();
    }

    async function startReview() {
      const all = state.review || [];
      const items = all.filter((m) => m.due).slice(0, 5);
      if (!items.length || state.busy) return;
      const refs = [];
      for (const m of items) for (const r of m.notes || []) if (!refs.some((x) => x.id === r.id) && refs.length < MOST_REFS) refs.push(r);
      state.refs = refs;
      drawRefs();
      const keys = items.map((m) => m.key);
      state.review = all.filter((m) => !keys.includes(m.key));
      send(reviewPrompt(items));
      let marked = false;
      try {
        marked = (await request('/api/review', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ done: keys }),
        })).ok;
      } catch (e) {
        marked = false;
      }
      if (!marked) {
        const now = state.review || [];
        state.review = [...items.filter((m) => !now.some((r) => r.key === m.key)), ...now];
      }
    }

    const TOOL_ICONS = {
      search_notes: '<circle cx="11" cy="11" r="6"/><path d="M20 20l-4.5-4.5"/>',
      open_note: '<path d="M7 3h7l5 5v13H7z"/><path d="M14 3v5h5"/>',
      connected_notes: '<circle cx="6" cy="7" r="2.2"/><circle cx="18" cy="6" r="2.2"/><circle cx="12" cy="17.5" r="2.2"/><path d="M7.4 8.9l3.5 6.7M16.9 7.9l-3.8 7.8"/>',
      edit_note: '<path d="M4 20h4L19 9l-4-4L4 16z"/>',
      create_note: '<path d="M12 5v14M5 12h14"/>',
      read_document: '<path d="M6 3h8l4 4v14H6z"/><path d="M9 12h6M9 16h6"/>',
      read_transcript: '<path d="M12 3a3 3 0 0 0-3 3v6a3 3 0 0 0 6 0V6a3 3 0 0 0-3-3z"/><path d="M5 11a7 7 0 0 0 14 0M12 18v3"/>',
      look_at_picture: '<rect x="3" y="5" width="18" height="14" rx="2"/><circle cx="9" cy="11" r="2"/><path d="M21 17l-5-5-8 7"/>',
      calculate: '<rect x="5" y="3" width="14" height="18" rx="2"/><path d="M8 7h8M8 11h2M12 11h2M16 11v6M8 15h2M12 15h2M8 18h2M12 18h2"/>',
      ask_user: '<circle cx="12" cy="12" r="9"/><path d="M9.5 9.5a2.5 2.5 0 1 1 3.5 2.3c-.6.3-1 .9-1 1.6V14M12 17.2v.1"/>',
      quiz: '<path d="M9 6h11M9 12h11M9 18h11"/><path d="M4 6l1 1 2-2M4 12l1 1 2-2M4 18l1 1 2-2"/>',
    };

    function stepRuns(steps) {
      const runs = [];
      for (const step of steps) {
        const item = typeof step === 'string' ? { text: step, tool: '', found: [] } : step;
        const last = runs.at(-1);
        if (last && item && last.step.text === item.text && !(item.found || []).length && !(last.step.found || []).length) last.times += 1;
        else runs.push({ step: item, times: 1 });
      }
      return runs;
    }

    function stepLine(step, working, times = 1) {
      const item = typeof step === 'string' ? { text: step, tool: '', found: [] } : step;
      const icon = working ? BLOT : `<svg class="msg-step-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${TOOL_ICONS[item.tool] || '<circle cx="12" cy="12" r="3"/>'}</svg>`;
      const label = `${icon}<span>${escape(working ? doing(item) : item.text || '')}</span>${times > 1 ? `<span class="msg-step-times" aria-label="${times} times">×${times}</span>` : ''}`;
      const found = (item.found || []).filter((f) => f && f !== item.text);
      if (!found.length || item.tool === 'open_note') return `<div class="msg-step">${label}</div>`;
      return `<details class="msg-step"><summary>${label}<span class="msg-step-count">${found.length}</span>${stepChevron}</summary><ul>${found.map((f) => `<li>${escape(f)}</li>`).join('')}</ul></details>`;
    }

    const proofMarks = new WeakMap();
    function proposalCard(p, i, j) {
      const where = `data-i="${i}" data-j="${j}"`;
      if (retiredPractice(p)) return '<div class="proposal-done">Interview practice is no longer part of leo.</div>';
      const waiting = p.state !== 'applied' && p.state !== 'dismissed' && state.messages.findLastIndex((m) => m.role === 'assistant') === i ? '<div class="proposal-wait"><i aria-hidden="true"></i>Waiting for you</div>' : '';
      if (p.kind !== 'create' && p.kind !== 'edit') {
        const acts = p.state === 'applied'
          ? `<div class="proposal-done">Done · <button class="msg-act" data-chat="undo" ${where}>Undo</button></div>`
          : p.state === 'dismissed'
            ? '<div class="proposal-done">Dismissed</div>'
            : `<div class="proposal-buttons"><button class="btn primary sm" data-chat="apply" ${where}>${p.kind === 'delete_note' || p.kind === 'delete_folder' ? 'Move to trash' : p.kind === 'delete_card' || p.kind === 'delete_board' ? 'Delete' : p.kind === 'restore_note' || p.kind === 'restore_folder' ? 'Bring it back' : 'Do it'}</button><button class="btn plain sm" data-chat="dismiss" ${where}>Dismiss</button></div>`;
        return `<div class="proposal organize ${escape(p.kind)}">${waiting}<div class="proposal-title">${escape(p.summary || 'A change')}</div>${typeof p.description === 'string' && (p.kind === 'update_task' || p.description) ? `<div class="proposal-why">${escape(p.description || 'Clear description')}</div>` : ''}${p.kind === 'add_margin_note' ? `<div class="proposal-why">“${escape(p.quote)}”: ${escape(p.text)}</div>` : ''}${acts}</div>`;
      }
      const done = p.state === 'applied'
        ? `<div class="proposal-done">${p.kind === 'create' ? 'Made' : 'Applied'} · <button class="msg-act" data-chat="open" data-id="${escape(p.made || p.note || '')}">Open the note</button><button class="msg-act" data-chat="undo" ${where}>Undo</button></div>`
        : p.state === 'dismissed'
          ? '<div class="proposal-done">Dismissed</div>'
          : `<div class="proposal-buttons"><button class="btn primary sm" data-chat="apply" ${where}>${p.kind === 'create' ? 'Create' : 'Apply'}</button><button class="btn plain sm" data-chat="dismiss" ${where}>Dismiss</button></div>`;
      if (p.kind === 'create') {
        const body = String(p.body || '');
        return `<div class="proposal">${waiting}<div class="proposal-title">New note: “${escape(p.title || 'Untitled')}”${p.folder ? ` in ${escape(p.folder)}` : ''}</div>
          <div class="proposal-body prose">${render(body.length > 900 ? `${body.slice(0, 900)}…` : body)}</div>${done}</div>`;
      }
      const old = p.find ? `<div class="proposal-old">${escape(p.find)}</div>` : '<div class="proposal-note">Added at the end:</div>';
      const marked = proofMarks.get(p);
      const parts = marked && marked.find === p.find && marked.replace === p.replace ? marked.parts : p.find ? root.leoMarkdown?.wordDiff?.(p.find, p.replace || '') : [{ kind: 'ins', text: p.replace || '' }];
      proofMarks.set(p, { find: p.find, replace: p.replace, parts });
      const change = parts
        ? `${p.find ? '' : old}<div class="proposal-marked">${parts.map(({ kind, text }) => (kind === 'del' ? `<del>${escape(text)}${DELETE_MARK}</del>` : kind === 'ins' ? `<ins>${CARET_MARK}${escape(text)}</ins>` : escape(text))).join('')}</div>`
        : `${old}<div class="proposal-new">${escape(p.replace || '')}</div>`;
      return `<div class="proposal">${waiting}<div class="proposal-title">Change to “${escape(p.title || 'a note')}”</div>${p.why ? `<div class="proposal-why">${escape(p.why)}</div>` : ''}
        ${change}${done}</div>`;
    }

    const inline = (text) => render(String(text || '')).replace(/^<p>/, '').replace(/<\/p>$/, '');

    function askCard(a, i, k, pending) {
      const where = `data-i="${i}" data-k="${k}"`;
      const answered = typeof a.answered === 'string';
      const off = answered || pending ? ' disabled' : '';
      const options = (a.options || []).map((o, n) => `<button type="button" class="ask-option${answered && a.answered === o ? ' chosen' : ''}" data-chat="ask-pick" ${where} data-o="${n}"${off}>${inline(o)}</button>`).join('');
      const hint = answered ? `You answered: ${escape(a.answered)}` : options ? 'Tap one, or type your own answer below.' : 'Type your answer below.';
      return `<div class="ask-card"><div class="ask-q prose">${inline(a.question)}</div>${options ? `<div class="ask-options">${options}</div>` : ''}<div class="ask-hint">${hint}</div></div>`;
    }

    const BLANK = 'QUIZBLANKMARK';

    function quizMark(q, said) {
      const ago = Date.now() - (markedAt.get(q) ?? 0);
      const writing = ago < QUIZ_MARK_MS ? ` class="writing" style="animation-delay: ${-ago}ms"` : '';
      const path = said === 'Your answer, not right' ? 'M4.6 4.2 Q9.4 9.3 14 14.2 M13.8 4.4 Q8.8 9 4.4 13.8' : 'M3.3 8.7 Q4.9 10.1 6.1 12.6 Q8.6 7.9 14 2.5';
      return `<svg class="quiz-mark" viewBox="0 0 18 18" role="img" aria-label="${said}"><path pathLength="1"${writing} d="${path}"/></svg>`;
    }

    function quizCard(q, i, k, pending, sources) {
      const where = `data-i="${i}" data-k="${k}"`;
      const done = typeof q.given === 'string';
      const reply = q.reply && typeof q.reply === 'object' ? q.reply : null;
      const marking = Boolean(reply && reply.pending);
      const off = done || pending || checking.has(q) ? ' disabled' : '';
      const asked = cite(q.kind === 'fill_blank'
        ? render(String(q.question || '').replace(/_{3,}/g, BLANK)).split(BLANK).join('<span class="quiz-blank" aria-label="blank"></span>')
        : render(String(q.question || '')), sources, escape);
      const draft = escape(done ? q.given : state.drafts[`${i}:${k}`] || '');
      let answer = '';
      if (q.kind === 'multiple_choice') {
        answer = `<div class="quiz-options">${(q.options || []).map((o, n) => {
          const mark = !done ? '' : plainAnswer(o) === plainAnswer(q.answer) ? ' right' : o === q.given ? ' wrong' : '';
          return `<button type="button" class="quiz-option${mark}" data-chat="quiz-pick" ${where} data-o="${n}"${off}><span class="quiz-letter">${String.fromCharCode(65 + n)}</span><span>${inline(o)}</span>${mark ? quizMark(q, mark === ' right' ? (o === q.given ? 'Your answer, right' : 'The right answer') : 'Your answer, not right') : ''}</button>`;
        }).join('')}</div>`;
      } else if (q.kind === 'fill_blank') {
        answer = `<div class="quiz-row"><input class="quiz-input" ${where} placeholder="The missing words" autocomplete="off" value="${draft}"${off}><button type="button" class="btn primary sm" data-chat="quiz-check" ${where}${off}>Check answer</button></div>`;
      } else {
        answer = `<div class="quiz-row column"><textarea class="quiz-input" ${where} rows="3" placeholder="Your answer, in your own words"${off}>${draft}</textarea><button type="button" class="btn primary sm" data-chat="quiz-check" ${where}${off}>Check answer</button></div>`;
      }
      const result = checking.has(q) ? `<div class="quiz-result">${BLOT}Checking your answer…</div>` : marking && !(reply && reply.text) ? `<div class="quiz-result">${BLOT}Felix is reading your answer…</div>` : '';
      const open = !done && !q.predict;
      const writing = thinkingOpen.has(q);
      const thinking = open && writing ? `<textarea class="quiz-thinking" ${where} rows="2" maxlength="${MOST_THINKING}" placeholder="What are you thinking? Half-formed is fine" aria-label="Your thinking"${off}>${escape(state.drafts[`think:${i}:${k}`] || '')}</textarea>` : '';
      const pct = Number.isFinite(q.surePct) ? q.surePct : null;
      const sure = open ? `<div class="quiz-sure"><label class="quiz-sure-meter${pct === null ? ' unset' : ''}" style="--pct: ${pct ?? 50}%"><span class="quiz-sure-label">How sure?</span><input type="range" class="quiz-sure-range" ${where} min="0" max="100" step="5" value="${pct ?? 50}" aria-label="How sure are you"${pct === null ? ' aria-valuetext="Not set"' : ''}${off}><output class="quiz-sure-pct">${pct === null ? '–' : `${pct}%`}</output></label><label class="quiz-think-toggle"><input type="checkbox" class="switch" role="switch" data-chat="quiz-think" ${where}${writing ? ' checked' : ''}${off}><span>Add your thinking</span></label></div>${thinking}` : '';
      const own = done && (sureText(q) || q.thinking) ? `<div class="quiz-own">${[sureText(q), q.thinking].filter(Boolean).map(escape).join(' · ')}</div>` : '';
      const hints = hintsOf(q);
      const hinted = hints
        .map((hint, n) => `<div class="quiz-hint prose">${hints.length > 1 ? `<span class="quiz-hint-n">Hint ${n + 1}</span>` : ''}${grade(hint.text || '').text ? cite(render(grade(hint.text).text), hint.sources || sources, escape, hint.files) : hint.pending ? '<span class="typing"><i></i><i></i><i></i></span>' : ''}</div>`)
        .join('');
      const waiting = hints.some((h) => h.pending);
      const askHint = open && !pending && !waiting ? `<button type="button" class="quiz-hint-ask" data-chat="quiz-hint" ${where}>${hints.length ? 'Another hint' : 'Get a hint'}</button>` : '';
      const marked = done && q.correct !== null && q.correct !== undefined;
      const judged = marked && !reply;
      const verdict = judged ? `<div class="quiz-verdict">${q.correct ? 'Right' : 'Not quite'}</div>` : '';
      const feedback = judged && q.feedback ? `<div class="quiz-explain prose">${cite(render(q.feedback), sources, escape)}</div>` : '';
      const why = `${verdict}${feedback}${judged && q.explain && !(q.feedback && q.kind !== 'multiple_choice') ? `<div class="quiz-explain">${cite(inline(q.explain), sources, escape)}</div>` : ''}`;
      const onward = q.held && marked && !marking ? `<div class="quiz-next"><button type="button" class="btn primary sm" data-chat="quiz-next" ${where}${state.busy ? ' disabled' : ''}>Next question</button>${reply ? '' : `<button type="button" class="btn plain sm" data-chat="quiz-why" ${where}${state.busy ? ' disabled' : ''}>Ask Felix ${q.correct ? 'why' : 'to explain'}</button>`}</div>` : '';
      const said = reply
        ? (grade(reply.text || '').text
          ? `<div class="quiz-reply prose">${cite(render(grade(reply.text).text), reply.sources || sources, escape, reply.files)}</div>`
          : '')
        : '';
      const tone = q.correct === true ? ' right' : q.correct === false ? ' wrong' : '';
      return `<div class="quiz-card${done ? ' done' : ''}${marking ? ' marking' : ''}${tone}"><div class="quiz-kind">${q.predict ? 'Predict · a guess, not marked' : escape(QUIZ_KINDS[q.kind] || 'Question')}</div><div class="quiz-q prose">${asked}</div>${hinted}${sure}${answer}${own}${askHint}${result}${why}${said}${onward}</div>`;
    }

    function askHint(i, k) {
      const m = state.messages[i];
      const q = m && m.quizzes && m.quizzes[k];
      if (!q || typeof q.given === 'string' || state.busy || m.pending || hintsOf(q).some((h) => h.pending)) return;
      send('Hint, please.', { say: hintSay(q), card: q, hint: true });
    }

    function answerAsk(i, k, given) {
      const m = state.messages[i];
      const a = m && m.asks && m.asks[k];
      if (!a || typeof a.answered === 'string' || state.busy) return;
      send(given);
    }

    const checking = new WeakSet();
    const thinkingOpen = new WeakSet();
    const markedAt = new WeakMap();

    async function answerQuiz(i, k, given) {
      const m = state.messages[i];
      const q = m && m.quizzes && m.quizzes[k];
      const text = String(given || '').trim();
      if (!q || typeof q.given === 'string' || state.busy || m.pending || !text || checking.has(q)) return;
      const thinking = thinkingOpen.has(q) ? String(state.drafts[`think:${i}:${k}`] || '').trim().slice(0, MOST_THINKING) : '';
      let correct = checkQuiz(q, text);
      if (correct === null && q.kind === 'fill_blank' && accepted(q).some((a) => plainAnswer(a) === plainAnswer(text))) correct = true;
      const known = correct;
      if ((known === null || thinking) && !q.predict) {
        const chatId = state.id;
        checking.add(q);
        draw();
        let mark = null;
        try {
          const response = await request('/api/quiz/mark', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ question: String(q.question || ''), answer: String(q.answer || ''), given: text, explain: String(q.explain || ''), confidence: SURE[q.sure] ? q.sure : '', thinking }) });
          if (response.ok) mark = await response.json();
        } catch (e) {}
        checking.delete(q);
        if (state.id !== chatId) return;
        if (mark && typeof mark.correct === 'boolean') {
          correct = known ?? mark.correct;
          q.feedback = String(mark.feedback || '');
        } else if (known === null && (state.busy || m.pending)) {
          draw();
          return;
        }
      }
      q.given = text;
      q.correct = correct;
      markedAt.set(q, Date.now());
      if (thinking) q.thinking = thinking;
      scoreSession(currentSession(), q, correct);
      delete state.drafts[`${i}:${k}`];
      delete state.drafts[`think:${i}:${k}`];
      if (correct === true) {
        state.streak += 1;
        mood(state.streak >= 3 ? 'cheer' : 'dance', state.streak >= 3 ? 2400 : 1800);
      } else if (correct === false) {
        state.streak = 0;
        mood('droop', 1600);
        setTimeout(() => mood('perk', 700), 1650);
      }
      if (correct === null) {
        send(text, { say: quizSay(q, text, correct), quiz: { question: q.question, given: text, correct, kind: q.kind }, card: q });
        return;
      }
      q.held = true;
      draw();
      remember();
    }

    function continueQuiz(i, k, why) {
      const m = state.messages[i];
      const q = m && m.quizzes && m.quizzes[k];
      if (!q || !q.held || state.busy) return;
      const quiz = { question: q.question, given: q.given, correct: q.correct, kind: q.kind };
      if (why) {
        if (q.reply) return;
        q.told = true;
        send(q.given, { say: quizSay(q, q.given, q.correct, true), quiz, card: q, held: true });
        return;
      }
      q.held = false;
      send('Next question', { say: heldSay(q, currentSession()?.state === 'finishing'), quiz, held: true, next: true });
    }

    async function postSteer(work, m) {
      if (!work.answerId || m.posted) return;
      m.posted = true;
      try {
        const response = await request(`/api/chat/${encodeURIComponent(work.answerId)}/steer`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ text: m.text }),
        });
        if (!response.ok) m.late = true;
      } catch (e) {
        m.late = true;
      }
    }

    function steerNow(text) {
      const said = text.trim();
      const work = state.busy;
      if (!said || !work) return;
      const m = { role: 'user', text: said, steer: 'waiting' };
      work.thread.messages.push(m);
      work.waiting.push(m);
      input.value = '';
      fit();
      draw();
      postSteer(work, m);
    }

    function steered(work, texts) {
      for (const text of texts) {
        const m = work.waiting.find((w) => w.steer === 'waiting' && text.startsWith(w.text.trim().slice(0, 32000)));
        if (!m) continue;
        m.steer = 'read';
        const list = work.thread.messages;
        list.splice(list.indexOf(m), 1);
        const at = list.indexOf(work.answer);
        list.splice(at < 0 ? list.length : at, 0, m);
      }
    }

    const inflight = new WeakSet();

    async function decide(i, j, apply) {
      const m = state.messages[i];
      const p = m && m.proposals && m.proposals[j];
      if (!p || p.state !== 'new' || inflight.has(p)) return;
      if (!apply) {
        p.state = 'dismissed';
        remember();
        draw(false);
        return;
      }
      if (p.kind !== 'create' && p.kind !== 'edit') return organize(p, p, true);
      const post = p.kind === 'create'
        ? ['/api/notes', { title: p.title, body: p.body || '', directory: p.folder || '' }]
        : [`/api/notes/${encodeURIComponent(p.note)}/suggestion`, { find: p.find || '', replace: p.replace || '' }];
      let note;
      inflight.add(p);
      try {
        const response = await request(post[0], {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify(post[1]),
        });
        if (!response.ok) {
          let said = '';
          try {
            said = (await response.json()).error || '';
          } catch (e) {
            said = '';
          }
          throw new Error(said || (p.kind === 'create' ? 'The note could not be made.' : 'The change could not be applied.'));
        }
        note = await response.json();
      } catch (e) {
        notify(e.message);
        return;
      } finally {
        inflight.delete(p);
      }
      p.state = 'applied';
      if (p.kind === 'create') {
        p.made = note.id;
        onSaved(note);
      } else {
        p.before = typeof note.before === 'string' ? note.before : null;
        p.after = note.version || null;
        onChanged(note);
      }
      remember();
      draw(false);
    }

    async function organize(p, action, forward) {
      if (retiredPractice(p) || inflight.has(p)) return;
      const sending = Object.fromEntries(Object.entries(action || {}).filter(([k]) => !['state', 'undo', 'task', 'path'].includes(k)));
      let done;
      const previous = p.state;
      inflight.add(p);
      try {
        {
          const response = await request('/api/felix/act', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(sending) });
          done = await response.json().catch(() => ({}));
          if (!response.ok) throw new Error(done.error || 'That could not be done.');
        }
      } catch (e) {
        p.state = previous; draw(false);
        notify(e.message || "Can't reach leo. Quit leo and open it again.");
        return;
      } finally {
        inflight.delete(p);
      }
      if (forward) {
        p.state = 'applied';
        p.undo = done.undo;
      } else {
        p.state = 'new';
        delete p.undo;
      }
      onOrganized(p);
      remember();
      draw(false);
    }

    async function undo(i, j) {
      const m = state.messages[i];
      const p = m && m.proposals && m.proposals[j];
      if (!p || p.state !== 'applied') return;
      if (p.kind !== 'create' && p.kind !== 'edit') {
        if (!p.undo) return notify('This change was made before leo could undo it.');
        return organize(p, p.undo, false);
      }
      const create = p.kind === 'create';
      if (!create && (p.before === null || p.before === undefined || !p.after)) {
        notify('This change was applied before leo could undo it; open the note to change it back.');
        return;
      }
      const [url, method, body] = create
        ? [`/api/notes/${encodeURIComponent(p.made)}`, 'DELETE', undefined]
        : [`/api/notes/${encodeURIComponent(p.note)}`, 'PATCH', JSON.stringify({ body: p.before, base: p.after })];
      let response;
      try {
        response = await request(url, { method, headers: body ? { 'Content-Type': 'application/json' } : {}, body });
      } catch (e) {
        notify("Can't reach leo. Quit leo and open it again.");
        return;
      }
      if (response.status === 409) {
        notify('The note was edited after this change, so leo did not undo it. Open the note to change it back.');
        return;
      }
      if (!response.ok && response.status !== 404) {
        notify('The change could not be undone.');
        return;
      }
      const note = create ? { id: p.made, title: p.title } : await response.json().catch(() => ({ id: p.note, title: p.title }));
      p.state = 'new';
      delete p.before;
      delete p.after;
      delete p.made;
      remember();
      draw(false);
      onUndone(note, create);
    }

    function addAnswer(i, picked) {
      const m = state.messages[i];
      if (!m || m.role !== 'assistant' || m.pending) return;
      const source = grade(m.text || '').text;
      const chosen = picked ? answerLines(source, picked) : source;
      const text = linkCitations(chosen, m.sources).trim();
      if (text) onAdd({ text, folder: m.folder || '' });
    }

    const addFloat = root.document.createElement('button');
    addFloat.type = 'button';
    addFloat.className = 'float-act';
    addFloat.dataset.chat = 'add-picked';
    addFloat.textContent = 'Add to note';
    addFloat.hidden = true;
    addFloat.addEventListener('mousedown', (e) => e.preventDefault());
    panel.appendChild(addFloat);

    function hideAddFloat() {
      addFloat.hidden = true;
      state.picked = null;
    }

    function pickedInAnswer() {
      const selection = root.getSelection ? root.getSelection() : null;
      if (!selection || selection.isCollapsed || !selection.rangeCount) return null;
      const range = selection.getRangeAt(0);
      const start = range.startContainer.nodeType === 1 ? range.startContainer : range.startContainer.parentElement;
      const end = range.endContainer.nodeType === 1 ? range.endContainer : range.endContainer.parentElement;
      const msg = start && start.closest('.msg.leo:not(.pending)');
      if (!msg || !body.contains(msg) || !end || end.closest('.msg.leo') !== msg || !start.closest('.prose') || !end.closest('.prose')) return null;
      const copy = range.cloneContents();
      for (const chip of copy.querySelectorAll('.cite, .katex-mathml, .msg-steps, .see-larger')) chip.remove();
      const text = copy.textContent || selection.toString();
      if (!text.trim()) return null;
      const box = range.getBoundingClientRect();
      return { i: Number(msg.dataset.i), text, box };
    }

    function placeAddFloat() {
      const picked = pickedInAnswer();
      if (!picked) return hideAddFloat();
      state.picked = picked;
      const area = panel.getBoundingClientRect();
      addFloat.hidden = false;
      const width = addFloat.offsetWidth;
      const left = Math.max(8, Math.min(picked.box.right - area.left - width / 2, area.width - width - 8));
      addFloat.style.left = `${left}px`;
      addFloat.style.top = `${picked.box.bottom - area.top + 6}px`;
    }

    root.document.addEventListener('selectionchange', () => {
      if (!state.open) return;
      clearTimeout(state.pickWait);
      state.pickWait = setTimeout(placeAddFloat, 120);
    });
    body.addEventListener('scroll', () => hideAddFloat(), { passive: true });
    body.addEventListener('click', (e) => {
      const trace = e.target.closest?.('.msg-trace > summary')?.parentElement;
      const m = trace && state.messages[Number(trace.dataset.i)];
      if (!m) return;
      if (trace.open) openTraces.delete(m);
      else openTraces.add(m);
    }, true);
    body.addEventListener('toggle', (e) => {
      if (!e.target.matches?.('.msg-trace')) return;
      const m = state.messages[Number(e.target.dataset.i)];
      if (!m) return;
      if (e.target.open) openTraces.add(m);
      else openTraces.delete(m);
    }, true);

    async function saveAnswer(i) {
      const m = state.messages[i];
      if (!m || m.role !== 'assistant' || m.saved || inflight.has(m)) return;
      let question = '';
      for (let j = i - 1; j >= 0; j--) {
        if (state.messages[j].role === 'user') {
          question = state.messages[j].text;
          break;
        }
      }
      const note = asNote(question, m.text, m.sources);
      const directory = typeof m.folder === 'string' ? m.folder : (state.context && state.context.directory) || '';
      let made;
      inflight.add(m);
      try {
        const response = await request('/api/notes', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ ...note, directory }),
        });
        if (!response.ok) throw new Error();
        made = await response.json();
      } catch (e) {
        notify('Felix could not save that answer as a note.');
        return;
      } finally {
        inflight.delete(m);
      }
      m.saved = made.id;
      remember();
      draw(false);
      onSaved(made);
    }

    function lastPair() {
      const n = state.messages.length;
      const a = state.messages[n - 1];
      const q = state.messages[n - 2];
      return a && q && a.role === 'assistant' && !a.pending && q.role === 'user' && !q.next && !q.quiz && !q.steer ? n - 2 : -1;
    }

    async function copyAnswer(i, button) {
      const m = state.messages[i];
      if (!m || m.role !== 'assistant' || m.pending || state.busy) return;
      try {
        await root.navigator.clipboard.writeText(linkCitations(grade(m.text || '').text, m.sources).trim());
        button.textContent = 'Copied';
        setTimeout(() => { button.textContent = 'Copy'; }, 1500);
      } catch (e) {
        notify(`Could not copy: ${e.message || 'the clipboard is not available.'}`);
      }
    }

    async function takeBack() {
      const at = lastPair();
      if (at < 0 || state.busy || state.preparingSend || !state.id) return null;
      const id = state.id;
      const asked = state.messages[at];
      state.preparingSend = true;
      try {
        await upload;
        const response = await request(`/api/chats/${encodeURIComponent(id)}/drop`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ count: 2, total: stored().length }) });
        if (!response.ok) throw new Error();
      } catch (e) {
        notify('Leo could not take back that reply. Try again.');
        return null;
      } finally {
        state.preparingSend = false;
      }
      if (state.id !== id) return null;
      state.messages.splice(at, 2);
      persist();
      draw(false);
      return asked;
    }

    function bringBack(asked) {
      const ids = asked.files || [];
      const back = state.sent.filter((f) => ids.includes(f.id));
      state.refs = [...(asked.refs || []), ...state.refs].reduce(addRef, []);
      if (asked.selection) state.selection = asked.selection;
      state.sent = state.sent.filter((f) => !back.includes(f));
      state.files = [...state.files, ...back.map((f) => ({ ...(asked.cards || []).find((c) => c.name === f.name), ...f, status: 'ready' }))];
      drawRefs();
    }

    async function retryAnswer() {
      const asked = await takeBack();
      if (!asked) return;
      bringBack(asked);
      send(asked.text);
    }

    async function editQuestion() {
      const asked = await takeBack();
      if (!asked) return;
      bringBack(asked);
      input.value = [asked.text, input.value].filter((t) => t.trim()).join('\n\n');
      fit();
      input.focus();
    }

    async function startWith(starter) {
      const ready = onNote() || state.refs.length > 0;
      if (!starter.needs || ready) {
        state.asking = null;
        return send(starter.text);
      }
      state.asking = { starter, folders: starter.needs === 'classes' ? null : [], picked: [] };
      draw();
      if (starter.needs === 'note') {
        openPick('button', '', 0);
        return;
      }
      let folders = [];
      try {
        const response = await request('/api/folders');
        if (response.ok) folders = (await response.json()).map((f) => f.name).filter((n) => n && !n.includes('/'));
      } catch (e) {
        folders = [];
      }
      if (state.asking && state.asking.starter === starter) {
        state.asking.folders = folders;
        draw();
      }
    }

    function answerClasses(all) {
      const a = state.asking;
      if (!a) return;
      const picked = all ? [] : a.picked;
      state.asking = null;
      send(all ? `${a.starter.text} for all my classes` : starterWords(a.starter, picked));
    }

    let liveFrame = 0;

    function draw(stick = true, live = false, later = false) {
      if (later && root.requestAnimationFrame) {
        if (!liveFrame) {
          liveFrame = root.requestAnimationFrame(() => {
            liveFrame = 0;
            draw(false, true);
          });
        }
        return;
      }
      if (liveFrame) root.cancelAnimationFrame?.(liveFrame);
      liveFrame = 0;
      const near = body.scrollHeight - body.scrollTop - body.clientHeight < 80;
      const typing = document.activeElement && document.activeElement.classList && document.activeElement.matches('.quiz-input, .quiz-thinking') && body.contains(document.activeElement)
        ? { box: document.activeElement.className.split(' ')[0], i: document.activeElement.dataset.i, k: document.activeElement.dataset.k, from: document.activeElement.selectionStart, to: document.activeElement.selectionEnd }
        : null;
      const kept = body.scrollTop;
      const session = currentSession();
      const recap = session && session.state === 'finished' ? recapCard(session) : '';
      const html = state.messages.length ? `${state.messages.map((m, i) => switches(state.messages, i) + bubble(m, i)).join('')}${recap}` : welcome();
      const perched = !state.messages.length;
      const land = perched !== panel.classList.contains('perched') && !panel.hidden && root.leoMotion?.dock ? root.leoMotion.dock($('.chat-foot'), $('#chat-form')) : null;
      const first = body.firstElementChild;
      const next = document.createElement('div');
      next.innerHTML = html;
      const hello = Boolean(first) && first.classList.contains('chat-hello') && Boolean(next.firstElementChild) && next.firstElementChild.dataset.hello === first.dataset.hello;
      if (hello || (live && body.querySelector(':scope > .msg'))) settle(body, next);
      else body.innerHTML = html;
      panel.classList.toggle('perched', perched);
      body.setAttribute('aria-busy', String(Boolean(state.busy)));
      document.getElementById('chat-toggle')?.classList.toggle('working', Boolean(state.busy));
      drawBelow();
      if (land) land();
      drawTitle();
      drawProgress();
      if (typing) {
        const again = body.querySelector(`.${typing.box}[data-i="${typing.i}"][data-k="${typing.k}"]`);
        if (again && !again.disabled) {
          again.focus({ preventScroll: true });
          again.setSelectionRange(typing.from, typing.to);
        }
      }
      body.scrollTop = !typing && (stick || near) ? body.scrollHeight : kept;
      drawSend();
    }

    function drawSend() {
      const stop = Boolean(state.busy) && !input.value.trim();
      const send = $('#chat-send');
      const shown = () => {
        send.classList.toggle('stop', stop);
        send.innerHTML = stop
          ? '<svg viewBox="0 0 24 24"><rect x="7" y="7" width="10" height="10" rx="2" fill="currentColor"/></svg>'
          : '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 19V5M6 11l6-6 6 6"/></svg>';
      };
      send.setAttribute('aria-label', stop ? 'Stop' : state.busy ? 'Send to Felix while he works' : 'Send');
      if (send.classList.contains('stop') === stop) shown();
      else if (root.leoMotion) root.leoMotion.iconSwap(send, shown);
      else shown();
      input.placeholder = state.busy ? 'Add to what Felix is doing…' : 'Message Felix, or type @ to add a note…';
    }

    function fit() {
      input.style.height = 'auto';
      input.style.height = `${Math.min(input.scrollHeight, 160)}px`;
    }

    function react(message) {
      const verdict = grade(message.text).verdict;
      if (verdict === 'correct') {
        state.streak += 1;
        mood(state.streak >= 3 ? 'cheer' : 'dance', state.streak >= 3 ? 2400 : 1800);
      } else if (verdict === 'incorrect') {
        state.streak = 0;
        mood('droop', 1600);
        setTimeout(() => mood('perk', 700), 1650);
      }
    }

    async function send(text, extra = {}) {
      const question = text.trim();
      if (!question || state.busy || state.preparingSend) return;
      if (!extra.held) {
        const held = state.messages.flatMap((m) => (m.quizzes || []).filter((q) => q.held && !q.told));
        if (held.length) {
          // The card keeps Next question: a question about the answer ("why?")
          // is answered, and the next practice question waits for the button.
          const results = held.map((q) => heldSay(q, false).split('\n').slice(0, 3).join('\n'));
          for (const q of held) q.told = true;
          const last = held[held.length - 1];
          const stay = extra.card ? [] : ['Answer my message. Do not ask a new practice question unless I ask for one.'];
          extra = { ...extra, say: [...results, ...stay, extra.say || question].join('\n'), quiz: extra.quiz || { question: last.question, given: last.given, correct: last.correct, kind: last.kind } };
        }
      }
      stopCompacting();
      const card = extra.card || null;
      if (!extra.quiz) {
        const last = [...state.messages].reverse().find((m) => m.role === 'assistant');
        for (const a of (last && last.asks) || []) if (typeof a.answered !== 'string') a.answered = question;
      }
      closePick();
      const ctx = state.context && state.dropped !== state.context.id ? state.context.id : null;
      let access = state.access;
      const going = card ? [] : state.files.filter((f) => f.status === 'ready' || f.status === 'reading');
      const attached = card ? [] : state.refs.slice();
      const passage = card ? null : state.selection;
      if (!card) {
        state.files = state.files.filter((f) => !going.includes(f));
        state.refs = [];
        state.selection = null;
        drawRefs();
      }
      const asked = { role: 'user', text: question, refs: attached, docs: [], files: [], cards: going.map(cardOf), ...(passage ? { selection: passage } : {}), ...(extra.say ? { say: extra.say } : {}), ...(extra.quiz ? { quiz: extra.quiz } : {}), ...(extra.next ? { next: true } : {}) };
      const settle = (i, doc) => {
        if (!doc) {
          asked.cards[i] = null;
          return;
        }
        asked.cards[i] = cardOf(doc);
        asked.files.push(doc.id);
        if (!doc.thumb) asked.docs.push(doc.name);
        state.sent = [...state.sent, { id: doc.id, name: doc.name }];
      };
      going.forEach((f, i) => {
        if (f.status === 'ready') settle(i, f);
      });
      const answer = { role: 'assistant', text: '', sources: [], pending: true, folder: (state.context && state.context.directory) || '' };
      working.set(answer, Date.now());
      if (card) answer.request = historyRequest(historyText(asked));
      if (card && extra.hint) card.hints = [...(Array.isArray(card.hints) ? card.hints : []), answer];
      else if (card) card.reply = answer;
      else {
        state.messages.push(asked);
        state.messages.push(answer);
        if (input.value.trim() === question) input.value = '';
        fit();
        drawChats();
      }
      const controller = new AbortController();
      const thread = { id: state.id, refs: state.refs.slice(), messages: state.messages };
      const work = { abort: () => controller.abort(), thread, answer, waiting: [], answerId: null, permissionWanted: state.access, permissionPosted: access, permissionWorking: false };
      state.busy = work;
      awake();
      mood('nod', 450);
      thinking(true);
      draw();
      const waiting = going.map((f, i) => [f, i]).filter(([f]) => f.status === 'reading');
      if (waiting.length) {
        const docs = await Promise.all(waiting.map(([f]) => reading.get(f.key) || Promise.resolve(null)));
        waiting.forEach(([, i], k) => settle(i, docs[k]));
        draw();
      }
      asked.cards = asked.cards.filter(Boolean);
      if (!asked.cards.length) delete asked.cards;
      const files = state.sent.map((f) => f.id);
      const history = modelHistory(state.messages.filter((m) => m !== answer));
      if (card) history.push({ role: 'user', text: answer.request });
      if (passage && history.length) history[history.length - 1] = { role: 'user', text: asked.say || asked.text };
      const sentRefs = threadRefs(thread.messages).map((r) => r.id);
      if (passage && passage.note && !sentRefs.includes(passage.note)) sentRefs.unshift(passage.note);
      try {
        access = state.access; work.permissionPosted = access;
        const response = await request('/api/chat', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ messages: history, note: ctx, refs: sentRefs.slice(0, MOST_REFS), recent: recentNotes(thread.messages.filter((m) => m !== answer)), chat: thread.id, files, access, practice: Boolean(card), mark: Boolean(card && !extra.hint && !card.predict && card.correct === null), selection: passage ? { title: passage.title, from: passage.from, to: passage.to, text: passage.text } : null, session: sessionBody(currentSession()) }),
          signal: controller.signal,
        });
        if (response.status === 401) throw new Error('leo needs to sign in again. Quit leo and open it again.');
        if (!response.ok || !response.body) {
          let said = '';
          try {
            said = (await response.json()).error || '';
          } catch (e) {
            said = '';
          }
          throw new Error(said || `leo answered ${response.status}.`);
        }
        const reader = response.body.getReader();
        const decoder = new TextDecoder();
        let buffer = '';
        for (;;) {
          const { value, done } = await reader.read();
          if (done) break;
          buffer += decoder.decode(value, { stream: true });
          const { events, rest } = splitLines(buffer);
          buffer = rest;
          for (const e of events) {
            if (typeof e.answer === 'string') {
              work.answerId = e.answer;
              postPermission(work);
              for (const m of work.waiting) postSteer(work, m);
            }
            if (Array.isArray(e.steered)) steered(work, e.steered);
            if (e.ask && typeof e.ask === 'object' && typeof e.ask.question === 'string') {
              answer.asks = [...(answer.asks || []), { question: e.ask.question, options: Array.isArray(e.ask.options) ? e.ask.options.filter((o) => typeof o === 'string') : [] }];
              pose('');
            }
            if (e.plan && typeof e.plan === 'object' && Array.isArray(e.plan.ideas)) {
              const before = currentSession();
              if (before && before.state !== 'finished') before.dismissed = true;
              answer.session = startSession(e.plan.ideas);
            }
            if (e.session_end === true) {
              const s = currentSession();
              if (s && s.state !== 'finished') s.state = 'finished';
            }
            if (e.quiz && typeof e.quiz === 'object' && typeof e.quiz.question === 'string') {
              // A new question moves the practice on: older cards lose Next question.
              for (const m of thread.messages) if (m !== answer) for (const q of m.quizzes || []) if (q.told) q.held = false;
              const idea = Number(e.quiz.idea) || 0;
              const last = thread.messages.flatMap((m) => (m === answer ? [] : m.quizzes || [])).findLast((q) => q.idea && !q.predict && typeof q.given === 'string');
              const retold = Boolean(last && idea === last.idea && (last.correct === false || (last === card && grade(answer.text).verdict === 'incorrect')));
              const s = currentSession();
              if (s && s.state === 'active' && idea && !e.quiz.predict) s.asked += 1;
              answer.quizzes = [...(answer.quizzes || []), { kind: String(e.quiz.kind || ''), question: e.quiz.question, options: Array.isArray(e.quiz.options) ? e.quiz.options.filter((o) => typeof o === 'string') : [], answer: String(e.quiz.answer || ''), explain: String(e.quiz.explain || ''), ...(idea ? { idea } : {}), ...(e.quiz.more || retold ? { more: true } : {}), ...(e.quiz.predict ? { predict: true } : {}) }];
            }
            if (e.reset) {
              answer.text = '';
              answer.steps = [];
              delete answer.quizzes;
              delete answer.asks;
              answer.proposals = (answer.proposals || []).filter((p) => p.state === 'applied');
            }
            if (e.verdict === 'correct' || e.verdict === 'incorrect' || e.verdict === 'ungraded') answer.verdict = e.verdict;
            if (e.sources) answer.sources = e.sources;
            if (Array.isArray(e.files)) answer.files = e.files;
            if (e.restart) answer.text = '';
            if (typeof e.step === 'string') pose(e.tool);
            if (typeof e.step === 'string') answer.steps = [...(answer.steps || []), { text: e.step, at: answer.text.length, tool: typeof e.tool === 'string' ? e.tool : '', failed: e.failed === true, found: Array.isArray(e.found) ? e.found.filter((f) => typeof f === 'string').slice(0, 12) : [] }];
            if (e.spent && typeof e.spent === 'object') answer.spent = e.spent;
            if (e.proposal && typeof e.proposal === 'object') {
              const done = e.proposal.state === 'applied';
              const { auto, ...shown } = e.proposal;
              const card = { ...shown, state: done ? 'applied' : 'new' };
              answer.proposals = [...(answer.proposals || []), card];
              if (done && e.proposal.kind === 'create' && e.proposal.made) onSaved({ id: e.proposal.made, title: e.proposal.title, directory: e.proposal.folder || '' });
              else if (done && e.proposal.kind === 'edit') onChanged({ id: e.proposal.note, title: e.proposal.title });
              else if (done) onOrganized(e.proposal);
            }
            if (typeof e.t === 'string') {
              if (!answer.text) talking();
              answer.text += e.t;
            }
            if (e.error) answer.error = e.error;
          }
          draw(false, true, true);
        }
      } catch (e) {
        if (e.name === 'AbortError') controller.abort();
        else answer.error = e.message || "Can't reach leo. Quit leo and open it again.";
      }
      answer.pending = false;
      answer.elapsed_ms = Math.max(0, Date.now() - (working.get(answer) ?? Date.now()));
      working.delete(answer);
      timed.add(answer);
      answer.at = new Date().toISOString();
      const cards = ['asks', 'quizzes', 'proposals'].some((key) => (answer[key] || []).length);
      const stopped = controller.signal.aborted || answer.error === 'Stopped.';
      if (stopped) answer.cancelled = true;
      if (stopped && !answer.error) answer.error = 'Stopped.';
      if (!answer.text && !answer.error && !cards) answer.error = 'Felix finished without an answer. Try again.';
      if (card) {
        const extras = ['quizzes', 'asks', 'proposals'].filter((key) => (answer[key] || []).length);
        if (extras.length) {
          const next = { role: 'assistant', text: '', sources: answer.sources, files: answer.files, at: answer.at };
          for (const key of extras) {
            next[key] = answer[key];
            delete answer[key];
          }
          thread.messages.push(next);
        }
        if (answer.verdict && grade(answer.text).verdict) {
          const match = /\[\[(correct|incorrect)\]\]/i.exec(answer.text);
          const replacement = answer.verdict === 'ungraded' ? '' : `[[${answer.verdict}]]`;
          const end = match.index + match[0].length;
          const delta = replacement.length - match[0].length;
          answer.text = answer.text.slice(0, match.index) + replacement + answer.text.slice(end);
          answer.steps = (answer.steps || []).map(step => ({ ...step, at: step.at >= end ? step.at + delta : step.at }));
        }
        if (!card.predict && (card.correct === null || card.correct === undefined)) {
          const verdict = answer.verdict || grade(answer.text).verdict;
          card.correct = verdict === 'correct' ? true : verdict === 'incorrect' ? false : null;
          scoreSession(currentSession(), card, card.correct);
        }
      }
      const ending = currentSession();
      if (ending && ending.state === 'finishing') ending.state = 'finished';
      const left = work.waiting.filter((m) => m.steer === 'waiting');
      for (const m of left) thread.messages.splice(thread.messages.indexOf(m), 1);
      for (const m of work.waiting) {
        delete m.posted;
        delete m.late;
      }
      state.busy = null;
      thinking(false);
      if (stopped && left.length && thread.id === state.id) {
        input.value = [input.value, ...left.map((m) => m.text)].filter((t) => t.trim()).join('\n\n');
        fit();
      }
      draw(false, true);
      remember({ ...thread, messages: thread.messages.slice() });
      if (thread.id === state.id) compactLater();
      if (thread.id === state.id && !(asked.quiz && asked.quiz.correct !== null && asked.quiz.correct !== undefined)) react(answer);
      awake();
      if (!stopped && left.length && thread.id === state.id) send(left.map((m) => m.text).join('\n\n'));
    }

    let leaving = 0;

    function toggle(force) {
      const was = state.open;
      state.open = force === undefined ? !state.open : force;
      if (!state.open) {
        historyResize.stop();
        light(null);
      }
      clearTimeout(leaving);
      const still = root.matchMedia && root.matchMedia('(prefers-reduced-motion: reduce)').matches;
      if (state.open) {
        panel.classList.remove('chat-leaving');
        panel.classList.toggle('perched', !state.messages.length);
        panel.hidden = false;
        document.body.classList.add('chat-open');
        onToggle(true);
      } else if (was && !panel.hidden && !still) {
        panel.classList.add('chat-leaving');
        document.body.classList.remove('chat-open');
        onToggle(false);
        leaving = setTimeout(() => {
          panel.hidden = true;
          panel.classList.remove('chat-leaving');
        }, CHAT_LEAVE_MS);
      } else {
        panel.hidden = true;
        document.body.classList.remove('chat-open');
        onToggle(false);
      }
      if (state.open) {
        syncSidebar(sidebarOpen());
        chatResize.fit();
        historyResize.fit();
        if (sidebarOpen()) loadChats();
        drawContext();
        if (state.filesFor !== state.id) loadFiles();
        drawRefs();
        draw();
        loadReview();
        mood('wave', 1500);
        awake();
        if (root.matchMedia && root.matchMedia('(pointer: fine)').matches) {
          if (sidebarOpen() && (!wide() || sourceNarrow())) history.querySelector('.chat-history-back').focus({ preventScroll: true });
          else input.focus();
        }
      }
    }

    panel.addEventListener('pointermove', gaze);
    panel.addEventListener('keydown', awake);
    panel.addEventListener('click', (e) => {
      awake();
      if (!e.target.closest('[data-chat^="attach"]')) attachMenu(false);
      if (!e.target.closest('[data-chat^="access"]')) {
        accessMenu(false);
      }
      if (e.target.closest('.felix') && !e.target.closest('[data-chat]')) {
        tapped();
        return;
      }
      const el = e.target.closest('[data-chat]');
      if (!el) return;
      e.preventDefault();
      const what = el.dataset.chat;
      if (what === 'close') toggle(false);
      else if (what === 'new') {
        begin();
        if (!wide() || sourceNarrow()) leaveOverlay();
      } else if (what === 'history') {
        if (panel.classList.contains('with-history')) closeHistory();
        else showSidebar(true);
      }
      else if (what === 'resume') resume(el.dataset.id);
      else if (what === 'save') saveAnswer(Number(el.dataset.i));
      else if (what === 'add') addAnswer(Number(el.dataset.i), '');
      else if (what === 'copy') copyAnswer(Number(el.dataset.i), el);
      else if (what === 'retry') retryAnswer();
      else if (what === 'edit') editQuestion();
      else if (what === 'add-picked') {
        const picked = state.picked;
        hideAddFloat();
        if (picked) addAnswer(picked.i, picked.text);
      }
      else if (what === 'session-finish') {
        const s = currentSession();
        if (s) s.state = 'finished';
        draw();
        remember();
      } else if (what === 'session-done') {
        const s = currentSession();
        if (s) s.dismissed = true;
        draw();
        remember();
      } else if (what === 'session-again') {
        const s = currentSession();
        if (!s) return;
        const c = sessionSummary(s);
        s.dismissed = true;
        const ideas = [...c.review, ...c.once, ...c.skipped];
        send(`Let's do a study session on just these ideas: ${ideas.join('; ')}.`);
      }
      else if (what === 'unselect') {
        state.selection = null;
        drawRefs();
      }
      else if (what === 'apply' || what === 'dismiss') decide(Number(el.dataset.i), Number(el.dataset.j), what === 'apply');
      else if (what === 'undo') undo(Number(el.dataset.i), Number(el.dataset.j));
      else if (what === 'ask-pick') {
        const m = state.messages[Number(el.dataset.i)];
        const a = m && m.asks && m.asks[Number(el.dataset.k)];
        if (a) answerAsk(Number(el.dataset.i), Number(el.dataset.k), a.options[Number(el.dataset.o)]);
      } else if (what === 'quiz-pick') {
        const m = state.messages[Number(el.dataset.i)];
        const q = m && m.quizzes && m.quizzes[Number(el.dataset.k)];
        if (q) answerQuiz(Number(el.dataset.i), Number(el.dataset.k), q.options[Number(el.dataset.o)]);
      } else if (what === 'quiz-next' || what === 'quiz-why') {
        continueQuiz(Number(el.dataset.i), Number(el.dataset.k), what === 'quiz-why');
      } else if (what === 'quiz-think') {
        const q = state.messages[Number(el.dataset.i)]?.quizzes?.[Number(el.dataset.k)];
        if (!q) return;
        if (thinkingOpen.has(q)) thinkingOpen.delete(q);
        else thinkingOpen.add(q);
        draw(false);
        if (thinkingOpen.has(q)) body.querySelector(`.quiz-thinking[data-i="${el.dataset.i}"][data-k="${el.dataset.k}"]`)?.focus();
      } else if (what === 'quiz-hint') {
        askHint(Number(el.dataset.i), Number(el.dataset.k));
      } else if (what === 'quiz-check') {
        const box = body.querySelector(`.quiz-input[data-i="${el.dataset.i}"][data-k="${el.dataset.k}"]`);
        if (box) answerQuiz(Number(el.dataset.i), Number(el.dataset.k), box.value);
      }
      else if (what === 'review') startReview();
      else if (what === 'forget') forget(el.dataset.id);
      else if (what === 'starter') startWith(STARTERS[Number(el.dataset.i)]);
      else if (what === 'ask-cancel') {
        state.asking = null;
        closePick();
        draw();
      } else if (what === 'class') {
        const a = state.asking;
        if (!a) return;
        const name = el.dataset.name;
        a.picked = a.picked.includes(name) ? a.picked.filter((n) => n !== name) : [...a.picked, name];
        draw();
      } else if (what === 'classes-go') answerClasses(false);
      else if (what === 'classes-all') answerClasses(true);
      else if (what === 'access') {
        const menu = $('#chat-access-menu');
        accessMenu(menu.hidden || Boolean(menu.dataset.folding));
      } else if (what === 'access-pick') {
        setAccess(el.dataset.access);
        input.focus();
      } else if (what === 'attach') {
        const menu = $('#chat-attach-menu');
        if (state.pick && state.pick.from === 'button') closePick();
        else if (!prepare) openPick('button', '', 0);
        else attachMenu(menu.hidden || Boolean(menu.dataset.folding));
      } else if (what === 'attach-note') {
        attachMenu(false);
        openPick('button', '', 0);
      } else if (what === 'attach-file') {
        attachMenu(false);
        if (state.files.length + state.sent.length >= MOST_FILES) notify(`A chat holds up to ${MOST_FILES} documents; remove one first.`);
        else $('#chat-file').click();
      } else if (what === 'unfile') removeFile(el.dataset.id); else if (what === 'pick') choose(Number(el.dataset.i));
      else if (what === 'unref') {
        state.refs = state.refs.filter((r) => r.id !== el.dataset.id);
        drawRefs();
        remember();
      }
      else if (what === 'drop') {
        state.dropped = state.dropped === state.context.id ? null : state.context.id;
        drawContext();
        if (!state.messages.length) draw();
      } else if (what === 'open') {
        if (!root.matchMedia || !root.matchMedia('(min-width: 900px)').matches) toggle(false);
        const [from, to] = (el.dataset.line || '').split('-').map(Number);
        onOpen(el.dataset.id, from ? { from, to: to || from } : null);
      } else if (what === 'original') {
        if (!root.matchMedia || !root.matchMedia('(min-width: 900px)').matches) toggle(false);
        onOriginal(el.dataset.note, el.dataset.name, el.dataset.at || '');
      }
    });
    $('#chat-form').addEventListener('submit', (e) => {
      e.preventDefault();
      if (state.busy && input.value.trim()) steerNow(input.value);
      else if (state.busy) state.busy.abort();
      else send(input.value);
    });
    body.addEventListener('input', (e) => {
      if (e.target.classList.contains('quiz-input')) state.drafts[`${e.target.dataset.i}:${e.target.dataset.k}`] = e.target.value;
      if (e.target.classList.contains('quiz-thinking')) state.drafts[`think:${e.target.dataset.i}:${e.target.dataset.k}`] = e.target.value;
      if (e.target.classList.contains('quiz-sure-range')) {
        const q = state.messages[Number(e.target.dataset.i)]?.quizzes?.[Number(e.target.dataset.k)];
        if (!q) return;
        q.surePct = Number(e.target.value);
        q.sure = sureOf(q.surePct);
        const meter = e.target.closest('.quiz-sure-meter');
        meter.classList.remove('unset');
        meter.style.setProperty('--pct', `${q.surePct}%`);
        meter.querySelector('.quiz-sure-pct').textContent = `${q.surePct}%`;
        e.target.removeAttribute('aria-valuetext');
      }
    });
    body.addEventListener('keydown', (e) => {
      if (!e.target.classList.contains('quiz-input') || e.key !== 'Enter' || e.shiftKey || e.isComposing) return;
      e.preventDefault();
      answerQuiz(Number(e.target.dataset.i), Number(e.target.dataset.k), e.target.value);
    });
    const carries = (e, type) => Boolean(e.dataTransfer && [...e.dataTransfer.types].includes(type));
    panel.addEventListener('dragover', (e) => {
      if (!carries(e, NOTE_DRAG) && !(prepare && carries(e, 'Files'))) return;
      e.preventDefault();
      e.dataTransfer.dropEffect = 'copy';
      panel.classList.add('drop-here');
    });
    panel.addEventListener('dragleave', (e) => {
      if (!panel.contains(e.relatedTarget)) panel.classList.remove('drop-here');
    });
    panel.addEventListener('drop', (e) => {
      panel.classList.remove('drop-here');
      let note = null;
      try {
        note = carries(e, NOTE_DRAG) ? JSON.parse(e.dataTransfer.getData(NOTE_DRAG)) : null;
      } catch (err) {
        note = null;
      }
      const found = prepare ? [...e.dataTransfer.files] : [];
      if (!note && !found.length) return;
      e.preventDefault();
      e.stopPropagation();
      if (note) {
        if (state.refs.length >= MOST_REFS && !state.refs.some((r) => r.id === note.id)) notify(`A message can bring up to ${MOST_REFS} notes; remove one first.`);
        state.refs = addRef(state.refs, note);
        drawRefs();
        remember();
      }
      if (found.length) {
        if (state.files.length + state.sent.length >= MOST_FILES) notify(`A chat holds up to ${MOST_FILES} documents; remove one first.`);
        else addFiles(pastedNames(found));
      }
      input.focus();
    });

    input.addEventListener('paste', (e) => {
      const found = [...((e.clipboardData && e.clipboardData.files) || [])];
      if (!found.length) return;
      e.preventDefault();
      if (state.files.length + state.sent.length >= MOST_FILES) {
        notify(`A chat holds up to ${MOST_FILES} documents; remove one first.`);
        return;
      }
      addFiles(pastedNames(found));
    });

    input.addEventListener('input', () => {
      fit();
      watchMention();
      drawSend();
    });
    input.addEventListener('click', watchMention);
    $('#chat-file').addEventListener('change', (e) => {
      attachMenu(false);
      addFiles(e.target.files);
      e.target.value = '';
    });
    function steer(e) {
      const p = state.pick;
      if (!p || !p.notes.length) return false;
      if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
        e.preventDefault();
        p.at = (p.at + (e.key === 'ArrowDown' ? 1 : p.notes.length - 1)) % p.notes.length;
        drawPick();
        return true;
      }
      if ((e.key === 'Enter' || e.key === 'Tab') && !e.isComposing) {
        e.preventDefault();
        choose(p.at);
        return true;
      }
      return false;
    }
    panel.addEventListener('input', (e) => {
      if (e.target.id === 'chat-pick-search') openPick('button', e.target.value, 0);
      if (e.target.id === 'chat-history-search') {
        clearTimeout(findTimer);
        const query = e.target.value;
        findTimer = setTimeout(() => findChats(query), 180);
      }
    });
    document.addEventListener('keydown', (e) => {
      if (!state.open || e.isComposing) return;
      const open = [['#chat-attach-menu', '[data-chat="attach"]'], ['#chat-access-menu', '#chat-access']]
        .map(([menu, trigger]) => ({ menu: $(menu), trigger: $(trigger) }))
        .find((x) => !x.menu.hidden && !x.menu.dataset.folding);
      if (e.key === 'Escape' && !open && !state.doomed && !state.busy && panel.classList.contains('with-history') && (history.contains(e.target) || !wide() || sourceNarrow()) && !e.target.closest('input, textarea') && !document.querySelector('.scrim')) {
        e.preventDefault();
        e.stopImmediatePropagation();
        closeHistory();
        return;
      }
      if (!open) return;
      const { menu, trigger } = open;
      if (e.key === 'Escape') {
        e.preventDefault();
        e.stopImmediatePropagation();
        popMenu(menu, false, trigger);
        trigger.setAttribute('aria-expanded', 'false');
        trigger.focus();
      } else if (['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(e.key) && (menu.contains(e.target) || e.target === trigger)) {
        const items = [...menu.querySelectorAll('button:not([hidden])')];
        if (!items.length) return;
        e.preventDefault();
        const at = items.indexOf(document.activeElement);
        let next;
        if (e.key === 'Home') next = 0;
        else if (e.key === 'End') next = items.length - 1;
        else if (at < 0) next = e.key === 'ArrowUp' ? items.length - 1 : 0;
        else next = (at + (e.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length;
        items[next].focus();
      }
    }, true);
    panel.addEventListener('keydown', (e) => {
      if (e.target.id !== 'chat-pick-search') return;
      if (e.key === 'Enter') e.preventDefault();
      if (e.key === 'Escape') {
        e.stopPropagation();
        closePick();
        input.focus();
      } else steer(e);
    });
    input.addEventListener('keydown', (e) => {
      if (e.key === 'Tab' && e.shiftKey && !state.pick) {
        e.preventDefault();
        setAccess(nextAccess(state.access));
        return;
      }
      if (state.pick && e.key === 'Escape') {
        e.stopPropagation();
        closePick();
        return;
      }
      if (state.pick && state.pick.from === 'mention' && steer(e)) return;
      if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) {
        e.preventDefault();
        if (state.busy) steerNow(input.value);
        else send(input.value);
      }
      if (e.key === 'Escape') {
        e.stopPropagation();
        if (state.busy) state.busy.abort();
        else if (panel.classList.contains('with-history')) closeHistory();
        else toggle(false);
      }
    });
    let lit = null;
    function light(from) {
      const chip = from && from.closest ? from.closest('.cite[data-chat="open"][data-line]') : null;
      if (chip === lit) return;
      lit = chip;
      if (!chip) return onCite(null, null);
      const [start, end] = chip.dataset.line.split('-').map(Number);
      onCite(chip.dataset.id, start ? { from: start, to: end || start } : null);
    }
    panel.addEventListener('pointerover', (e) => light(e.target));
    panel.addEventListener('pointerout', (e) => light(e.relatedTarget));
    panel.addEventListener('focusin', (e) => light(e.target));
    panel.addEventListener('focusout', (e) => light(e.relatedTarget));
    window.addEventListener('keydown', (e) => {
      if (e.key !== 'Escape' || e.isComposing || !state.open || e.target !== document.body || document.querySelector('.scrim, .scrim-gone')) return;
      e.preventDefault();
      e.stopImmediatePropagation();
      if (!state.doomed && !state.busy && panel.classList.contains('with-history')) closeHistory();
      else if (state.busy) state.busy.abort();
      else toggle(false);
    });
    panel.addEventListener('keydown', (e) => {
      if (e.key !== 'Escape' || e.target.closest('input, textarea, select, [contenteditable]')) return;
      e.stopPropagation();
      if (state.busy) state.busy.abort();
      else toggle(false);
    });

    return {
      toggle,
      attachSelection(selection) {
        if (!selection || typeof selection.text !== 'string' || !selection.text.trim()) return;
        state.selection = { note: String(selection.note || ''), title: String(selection.title || ''), from: Number(selection.from) || 0, to: Number(selection.to) || 0, text: selection.text };
        toggle(true);
        drawRefs();
        input.focus();
      },
      attachFiles(list) {
        toggle(true);
        addFiles(pastedNames([...list]));
        input.focus();
      },
      isOpen: () => state.open,
      isBusy: () => Boolean(state.busy || state.preparingSend),
      askNew(text) {
        const said = String(text || '').trim();
        if (!said || state.busy || state.preparingSend) return false;
        begin();
        toggle(true);
        send(said);
        return true;
      },
      async openSaved(id) {
        if (state.busy || state.preparingSend) return false;
        const opened = await resume(id);
        if (opened) toggle(true);
        return opened;
      },
      forgetBrowserCache() {
        if (state.busy || state.preparingSend) return false;
        begin();
        input.value = '';
        state.drafts = {};
        storage.removeItem(KEY);
        return true;
      },
      send,
      setContext(ctx) {
        const next = ctx && ctx.id ? { id: ctx.id, title: ctx.title, directory: ctx.directory || '' } : null;
        if ((next && next.id) === (state.context && state.context.id) && (!next || next.title === state.context.title)) return;
        state.context = next;
        if (state.open) {
          drawContext();
          if (!state.messages.length) draw();
        }
      },
      button: (width) => felix(width, 'idle'),
    };
  }

  root.leoChat = { noAi, retiredPractice, settle, timeline, startSession, scoreSession, sessionBody, sessionSummary, linkCitations, numbered, answerLines, passageLine, historyText, modelHistory, historyRequest, checkQuiz, quizSay, hintSay, plainAnswer, QUIZ_KINDS, held, recentNotes, threadRefs, ACCESS, accessOf, nextAccess, NOTE_DRAG, spentLabel, usageKinds, usagePeriod, usageRank, usageFold, traceSummary, doing, notFromNotes, fileKind, fileCard, create, felix, splitLines, grade, cite, cited, load, save, mentionAt, addRef, groups, newId, starterWords, splitFiles, asNote, reviewPrompt, pastedNames, poseOf, STARTERS, SURE, MOST_REFS, TAPS, DEMON_TAPS, MONSTER_ONE_IN, TICKLED_TOO_LONG, heldSay, sureOf, noteCard, noteExcerpt, fixedSteps };
})(typeof window !== 'undefined' ? window : globalThis);
