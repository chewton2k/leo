(function (root) {
  'use strict';

  const md = root.leoMarkdown;
  const ed = root.leoEditing;

  function mount(container, { source, onChange, placeholder, dir = '', onPictures = null, onSelection = null, outline = null }) {
    let lines = String(source || '').replace(/\r\n?/g, '\n').split('\n');
    let blocks = [];
    let rendered = [];
    let editing = null;

    const MOST_UNDO = 200;
    const TYPING_PAUSE = 1000;
    const TYPING_GROUP = 4000;
    const past = [];
    const future = [];
    let current = lines.join('\n');
    let typing = { in: null, last: 0, since: 0 };

    function changed() {
      const next = lines.join('\n');
      if (next !== current) {
        const now = Date.now();
        const keystroke = Math.abs(next.length - current.length) <= 1;
        const same = keystroke && editing && typing.in === editing.area && now - typing.last < TYPING_PAUSE && now - typing.since < TYPING_GROUP;
        if (!same) {
          past.push(current);
          if (past.length > MOST_UNDO) past.shift();
          typing = { in: keystroke && editing ? editing.area : null, last: now, since: now };
        } else {
          typing.last = now;
        }
        future.length = 0;
        current = next;
      }
      container.classList.toggle('doc-empty', !next.trim());
      onChange(next);
      outlineLater();
    }

    function restore(text) {
      const before = current;
      stop();
      typing = { in: null, last: 0, since: 0 };
      current = text;
      lines = text.split('\n');
      onChange(text);
      outlineLater();
      draw();
      let from = 0;
      while (from < before.length && from < text.length && before[from] === text[from]) from++;
      let tail = 0;
      while (tail < before.length - from && tail < text.length - from && before[before.length - 1 - tail] === text[text.length - 1 - tail]) tail++;
      const to = Math.max(from, text.length - tail);
      if (lineOf(from) !== lineOf(to)) selectSpan(from, to);
      else editSpan(from, to);
    }

    function undo() {
      if (!past.length) return false;
      if (editing) stop();
      future.push(current);
      restore(past.pop());
      return true;
    }

    function redo() {
      if (!future.length) return false;
      if (editing) stop();
      past.push(current);
      restore(future.pop());
      return true;
    }

    function historyKey(e) {
      if (!(e.metaKey || e.ctrlKey) || e.altKey) return false;
      const key = e.key.toLowerCase();
      if (key === 'z') {
        e.preventDefault();
        if (e.shiftKey) redo();
        else undo();
        return true;
      }
      if (key === 'y' && e.ctrlKey && !e.metaKey) {
        e.preventDefault();
        redo();
        return true;
      }
      return false;
    }

    function listStarts(levels) {
      const firsts = [];
      return levels.map((level, i) => {
        if (level < 0) {
          firsts.length = 0;
          return undefined;
        }
        firsts.length = level + 1;
        const found = lines[i].match(/^\s*(\d+)[.)]/);
        if (!found) {
          firsts[level] = null;
          return undefined;
        }
        if (firsts[level] == null) firsts[level] = Number(found[1]);
        return firsts[level];
      });
    }

    function renderBlock(block, boxes, levels, starts) {
      const text = lines.slice(block.start, block.end).join('\n');
      if (!text.trim()) {
        const lone = lines.length === 1;
        return lone ? `<p class="placeholder">${md.escape(placeholder || 'Start writing, or type # for a heading and - for a list')}</p>` : '<p class="blank">&#8203;</p>';
      }
      return md.render(text.replace(/^\s+/, ''), {
        boxOffset: boxes,
        dir,
        depth: block.kind === 'line' ? Math.max(levels[block.start], 0) + 1 : 1,
        after: block.kind === 'line' && levels[block.start - 1] >= 0,
        first: block.kind === 'line' ? starts[block.start] : undefined,
      });
    }

    function draw() {
      blocks = ed.splitBlocks(lines);
      const levels = ed.listLevels(lines);
      const starts = listStarts(levels);
      let boxes = 0;
      rendered = blocks.map((block, i) => {
        const text = lines.slice(block.start, block.end).join('\n');
        const level = block.kind === 'line' ? levels[block.start] : 0;
        const before = boxes;
        for (let at = block.start; at < block.end && at < lines.length; at++) if (ed.isBox(lines[at])) boxes++;
        const key = JSON.stringify([block.kind, text, before, level]);
        const old = rendered[i];
        if (old && old.key === key && old.el.isConnected && !old.el.classList.contains('editing')) {
          old.el.dataset.start = String(block.start);
          return old;
        }
        const el = document.createElement('div');
        el.className = `blk blk-${block.kind}`;
        el.dataset.start = String(block.start);
        el.dataset.i = String(i);
        if (level > 0) el.style.marginLeft = `${level * 1.5}em`;
        el.innerHTML = renderBlock(block, before, levels, starts);
        if (old && old.el.parentNode === container) old.el.replaceWith(el);
        else container.insertBefore(el, container.querySelector('.doc-tail'));
        return { key, el };
      });
      for (const el of container.querySelectorAll('.blk')) {
        if (Number(el.dataset.i) >= blocks.length) el.remove();
      }
      container.classList.toggle('doc-empty', !lines.join('').trim());
      if (!container.querySelector('.doc-tail')) {
        const tail = document.createElement('div');
        tail.className = 'doc-tail';
        tail.dataset.tail = '1';
        container.appendChild(tail);
      }
      outlineScroll();
    }

    let outlineTimer = 0;
    let outlineFrame = 0;
    let outlineKey = '';
    let outlineWide = null;
    let outlineRows = [];
    const outlineFolded = new Set();
    const outlineChevron = outline && outline.querySelector('summary > svg') ? outline.querySelector('summary > svg').outerHTML : '';
    const outlineNav = outline && outline.querySelector('nav');
    const outlineDetails = outline && outline.querySelector('details');
    const outlineTicks = outlineNav ? document.createElement('button') : null;
    const outlinePreview = outlineNav ? document.createElement('div') : null;
    const outlineUpArrow = outlineNav ? document.createElement('button') : null;
    const outlineDownArrow = outlineNav ? document.createElement('button') : null;
    let outlineStart = 0;
    let outlineFollowed = null;
    let outlineIndex = null;
    let outlineMotion = null;
    let outlineClosing = false;
    let outlinePointerType = '';
    let outlinePreviewMotion = null;
    let outlinePreviewClosing = false;
    if (outlineTicks) {
      outlineTicks.className = 'outline-ticks';
      outlineTicks.type = 'button';
      outlineTicks.setAttribute('aria-label', 'Jump to heading: Headings');
      outlineTicks.setAttribute('aria-describedby', 'outline-preview');
      outlinePreview.id = 'outline-preview';
      outlinePreview.className = 'outline-preview';
      outlinePreview.setAttribute('role', 'tooltip');
      outlinePreview.hidden = true;
      for (const [arrow, label, path] of [[outlineUpArrow, 'Earlier headings', 'M2.2 8.1 Q5.4 4.6 8.1 2.3 Q10.9 4.9 13.8 7.7'], [outlineDownArrow, 'Later headings', 'M2.1 2.2 Q5.3 5.6 8 7.8 Q10.8 5.2 13.9 2.4']]) {
        arrow.type = 'button';
        arrow.className = 'outline-page';
        arrow.hidden = true;
        arrow.setAttribute('aria-label', label);
        arrow.title = label;
        arrow.innerHTML = `<svg viewBox="0 0 16 10" width="16" height="10" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="${path}"/></svg>`;
      }
      outline.append(outlineUpArrow, outlineTicks, outlineDownArrow, outlinePreview);
    }
    const OUTLINE_PAUSE = 300;
    const OUTLINE_PAGE = 24;
    const OUTLINE_WIDE = 900;
    const MOST_PREVIEW_LINES = 24;
    const MOST_PREVIEW_CHARS = 800;
    const OUTLINE_MOTION_MS = 240;
    const OUTLINE_EXIT_MS = OUTLINE_MOTION_MS * .8;
    const outlineEase = () => getComputedStyle(document.documentElement).getPropertyValue('--ease-arrive').trim() || 'ease-out';

    function closeOutline() {
      outlineMotion?.cancel();
      outlineMotion = null;
      outlineClosing = false;
      outlineDetails.open = false;
    }

    function toggleOutline(e) {
      if (!e.target.closest('summary')) return false;
      e.preventDefault();
      outlinePeek(null);
      if (!e.detail || outlinePointerType !== 'mouse' || !outlineDetails.open || outlineClosing) moveOutline(!outlineDetails.open || outlineClosing);
      return true;
    }

    function moveOutline(open) {
      const from = outlineMotion ? { clipPath: getComputedStyle(outlineNav).clipPath, transform: getComputedStyle(outlineNav).transform } : null;
      outlineMotion?.cancel();
      outlineMotion = null;
      outlineClosing = !open;
      const still = root.matchMedia?.('(prefers-reduced-motion: reduce)').matches;
      if (still || typeof outlineNav.animate !== 'function') {
        outlineDetails.open = open;
        outlineClosing = false;
        return true;
      }
      const closed = { clipPath: 'inset(0 0 100% 0)', transform: 'scale(.95)' };
      const shown = { clipPath: 'inset(0 0 0 0)', transform: 'none' };
      outlineDetails.open = true;
      const run = outlineNav.animate([from || (open ? closed : shown), open ? shown : closed], { duration: open ? OUTLINE_MOTION_MS : OUTLINE_EXIT_MS, easing: outlineEase() });
      outlineMotion = run;
      run.onfinish = () => {
        if (outlineMotion !== run) return;
        outlineDetails.open = open;
        outlineClosing = false;
        outlineMotion = null;
      };
    }

    function outlineShowPreview(open) {
      if (!open && outlinePreview.hidden || open && !outlinePreview.hidden && !outlinePreviewClosing) return;
      const from = outlinePreviewMotion ? { opacity: getComputedStyle(outlinePreview).opacity, transform: getComputedStyle(outlinePreview).transform } : null;
      outlinePreviewMotion?.cancel();
      outlinePreviewMotion = null;
      outlinePreviewClosing = !open;
      if (root.matchMedia?.('(prefers-reduced-motion: reduce)').matches || typeof outlinePreview.animate !== 'function') {
        outlinePreview.hidden = !open;
        outlinePreviewClosing = false;
        return;
      }
      const closed = { opacity: 0, transform: 'translateX(-6px) scale(.95)' };
      const shown = { opacity: 1, transform: 'none' };
      outlinePreview.hidden = false;
      const run = outlinePreview.animate([from || (open ? closed : shown), open ? shown : closed], { duration: open ? OUTLINE_MOTION_MS : OUTLINE_EXIT_MS, easing: outlineEase(), fill: 'both' });
      outlinePreviewMotion = run;
      run.onfinish = () => {
        if (outlinePreviewMotion !== run) return;
        outlinePreview.hidden = !open;
        outlinePreviewClosing = false;
        outlinePreviewMotion = null;
        run.cancel();
      };
    }

    function outlineDown(e) {
      outlinePointerType = e.pointerType;
    }

    function outlineEnter(e) {
      if (e.pointerType !== 'mouse' || e.buttons) return;
      outlinePeek(null);
      if (!outlineDetails.open || outlineClosing) moveOutline(true);
    }

    function outlineExit(e) {
      if (e.pointerType !== 'mouse' || outlineDetails.contains(e.relatedTarget)) return;
      if (outlineDetails.open && !outlineClosing) moveOutline(false);
    }

    function outlineOutside(e) {
      if (outline.contains(e.target)) return;
      if (outlineDetails.open && !outlineClosing) moveOutline(false);
      outlinePeek(null);
    }

    function outlinePeek(index) {
      outlineIndex = index;
      for (const [i, tick] of Array.from(outlineTicks.children).entries()) {
        if (index === null) tick.removeAttribute('data-distance');
        else tick.dataset.distance = String(Math.min(3, Math.abs(i - index)));
      }
      const row = index === null ? null : outlineRows[index];
      const room = row ? outlineRoom() : null;
      outlineShowPreview(Boolean(room));
      if (!row) return;
      if (outlineWide) closeOutline();
      const title = md.plain(row.text);
      const end = Math.min(outlineRows[index + 1]?.line ?? lines.length, row.line + 1 + MOST_PREVIEW_LINES);
      let excerpt = '';
      for (let line = row.line + 1; line < end && excerpt.length < MOST_PREVIEW_CHARS; line++) {
        for (const char of lines[line] + '\n') {
          if (excerpt.length + char.length > MOST_PREVIEW_CHARS) break;
          excerpt += char;
        }
      }
      const text = md.plain(excerpt);
      outlinePreview.innerHTML = `<strong>${md.escape(title)}</strong>${text ? `<span>${md.escape(text)}</span>` : ''}`;
      outlineTicks.setAttribute('aria-label', `Jump to heading: ${title}`);
      if (!room) return;
      outlinePreview.style.left = `${room.left}px`;
      outlinePreview.style.width = `${room.width}px`;
      outlinePreview.style.top = '0px';
      const tick = outlineTicks.children[index].getBoundingClientRect();
      const rect = outline.getBoundingClientRect();
      const height = outlinePreview.getBoundingClientRect().height;
      const top = Math.max(0, Math.min(tick.top + tick.height / 2 - rect.top - height / 2, root.innerHeight - rect.top - height - 12));
      outlinePreview.style.top = `${top}px`;
    }

    function outlineRoom() {
      const rail = outline.getBoundingClientRect();
      const end = Math.min(root.innerWidth, outline.parentElement.getBoundingClientRect().right) - 12;
      return { left: rail.width + 12, width: Math.min(280, end - rail.right - 12) };
    }

    function outlinePointer(y) {
      const rect = outlineTicks.getBoundingClientRect();
      const shown = Math.min(OUTLINE_PAGE, outlineRows.length - outlineStart);
      return Math.max(outlineStart, Math.min(outlineStart + shown - 1, outlineStart + Math.floor((y - rect.top) / rect.height * shown)));
    }

    function outlinePage(start) {
      outlineStart = Math.max(0, Math.min(Math.floor(start / OUTLINE_PAGE) * OUTLINE_PAGE, Math.floor(Math.max(0, outlineRows.length - 1) / OUTLINE_PAGE) * OUTLINE_PAGE));
      for (const [i, tick] of Array.from(outlineTicks.children).entries()) tick.hidden = i < outlineStart || i >= outlineStart + OUTLINE_PAGE;
      outlineTicks.style.setProperty('--outline-count', Math.min(OUTLINE_PAGE, outlineRows.length - outlineStart));
      outlineUpArrow.hidden = outlineStart === 0;
      outlineDownArrow.hidden = outlineStart + OUTLINE_PAGE >= outlineRows.length;
    }

    function outlineEscape(e) {
      if (e.key !== 'Escape' || outlineIndex === null) return;
      e.preventDefault();
      e.stopPropagation();
      outlinePeek(null);
    }

    function outlineHover(e) {
      if (e.pointerType === 'touch' || e.buttons) return;
      const index = outlinePointer(e.clientY);
      if (index !== outlineIndex) outlinePeek(index);
    }

    function outlineLeave() {
      if (document.activeElement !== outlineTicks) outlinePeek(null);
    }

    function outlineFocus() {
      if (outlineIndex !== null) return;
      const current = outlineRows.findIndex(row => String(row.line) === outlineTicks.querySelector('.on')?.dataset.tick);
      outlinePeek(Math.max(0, current));
    }

    function outlineBlur() {
      outlinePeek(null);
    }

    function outlineKeydown(e) {
      const index = outlineIndex ?? 0;
      const next = { ArrowDown: Math.min(outlineRows.length - 1, index + 1), ArrowUp: Math.max(0, index - 1), Home: 0, End: outlineRows.length - 1 }[e.key];
      if (next !== undefined) {
        e.preventDefault();
        if (next < outlineStart || next >= outlineStart + OUTLINE_PAGE) outlinePage(next);
        outlinePeek(next);
      }
    }

    function outlineTop() {
      const bar = document.getElementById('bar');
      return (bar ? bar.getBoundingClientRect().bottom : 0) + 12;
    }

    function outlineActive() {
      outlineFrame = 0;
      if (!outline || !outline.isConnected || outline.hidden) return;
      const top = outlineTop();
      outline.style.setProperty('--outline-top', `${top}px`);
      outline.style.setProperty('--outline-center', `${Math.max(top, (root.innerHeight - outline.offsetHeight) / 2)}px`);
      let current = outlineRows[0];
      const bottom = root.scrollY > 0 && root.scrollY + root.innerHeight >= document.documentElement.scrollHeight - 2;
      for (const row of outlineRows) {
        const block = container.querySelector(`.blk[data-start="${row.line}"]`);
        if (block && block.getBoundingClientRect().top <= (bottom ? root.innerHeight : top + 24)) current = row;
      }
      let selected = current && outlineNav.querySelector(`button[data-line="${current.line}"]`);
      let hidden = selected && selected.closest('ul[hidden]');
      while (hidden) {
        selected = outlineNav.querySelector(`button[data-line="${hidden.dataset.parent}"]`);
        hidden = selected && selected.closest('ul[hidden]');
      }
      for (const button of outlineNav.querySelectorAll('button[data-line]')) {
        if (button === selected) button.setAttribute('aria-current', 'location');
        else button.removeAttribute('aria-current');
      }
      for (const tick of outlineTicks.children) tick.classList.toggle('on', Boolean(current) && tick.dataset.tick === String(current.line));
      const at = current ? outlineRows.indexOf(current) : 0;
      if (current?.line !== outlineFollowed) {
        outlineFollowed = current?.line;
        if (at < outlineStart || at >= outlineStart + OUTLINE_PAGE) outlinePage(at);
      }
      if (outlineIndex !== null) outlinePeek(outlineIndex);
    }

    function outlineScroll() {
      if (outline && !outlineFrame) outlineFrame = requestAnimationFrame(outlineActive);
    }

    function outlineList(rows, parent = null) {
      const folded = parent && outlineFolded.has(parent.key);
      const group = parent ? ` id="outline-group-${parent.line}" data-parent="${parent.line}"${folded ? ' hidden' : ''}` : '';
      return `<ul${group}>${rows.map((row) => {
        const text = md.escape(md.plain(row.text));
        const children = row.children.length > 0;
        const toggle = children ? `<button type="button" class="outline-toggle" data-fold="${row.line}" aria-label="Subheadings of ${text}" aria-expanded="${!outlineFolded.has(row.key)}" aria-controls="outline-group-${row.line}">${outlineChevron}</button>` : '<span class="outline-spacer" aria-hidden="true"></span>';
        return `<li><div class="outline-row">${toggle}<button type="button" data-line="${row.line}" title="${text}">${text}</button></div>${children ? outlineList(row.children, row) : ''}</li>`;
      }).join('')}</ul>`;
    }

    function outlineDraw(force = false) {
      if (!outlineNav || !container.isConnected) return;
      outlineRows = ed.headings(lines);
      const repeats = new Map();
      for (const row of outlineRows) {
        const label = JSON.stringify([row.level, row.text]);
        const occurrence = repeats.get(label) || 0;
        repeats.set(label, occurrence + 1);
        row.key = JSON.stringify([label, occurrence]);
      }
      const keys = new Set(outlineRows.map((row) => row.key));
      for (const key of outlineFolded) if (!keys.has(key)) outlineFolded.delete(key);
      outline.hidden = !outlineRows.length;
      const key = JSON.stringify(outlineRows);
      if (force || key !== outlineKey) {
        const active = outlineNav.contains(document.activeElement) ? document.activeElement : null;
        const focused = active ? { type: active.hasAttribute('data-fold') ? 'fold' : 'line', line: active.dataset.fold || active.dataset.line } : null;
        outlineKey = key;
        outlineNav.innerHTML = outlineList(ed.headingTree(outlineRows));
        outlinePeek(null);
        outlineTicks.innerHTML = outlineRows.map((row) => `<span aria-hidden="true" data-tick="${row.line}" data-level="${Math.min(row.level, 3)}"></span>`).join('');
        outlinePage(outlineStart);
        if (focused) {
          const button = outlineNav.querySelector(`[data-${focused.type}="${focused.line}"]`) || outlineNav.querySelector('button[data-line]');
          if (button) button.focus({ preventScroll: true });
        }
      }
      outlineScroll();
    }

    function outlineLater() {
      if (!outline) return;
      clearTimeout(outlineTimer);
      outlineTimer = setTimeout(() => outlineDraw(), OUTLINE_PAUSE);
    }

    function outlineGo(e) {
      if (toggleOutline(e)) return;
      const toggle = e.target.closest('button[data-fold]');
      if (toggle && outlineNav.contains(toggle)) {
        toggle.focus({ preventScroll: true });
        const row = outlineRows.find((row) => row.line === Number(toggle.dataset.fold));
        if (row) {
          if (outlineFolded.has(row.key)) outlineFolded.delete(row.key);
          else outlineFolded.add(row.key);
          outlineDraw(true);
        }
        return;
      }
      const arrow = e.target.closest('.outline-page');
      if (arrow) {
        outlinePage(outlineStart + (arrow === outlineUpArrow ? -OUTLINE_PAGE : OUTLINE_PAGE));
        (arrow.hidden ? outlineTicks : arrow).focus({ preventScroll: true });
        return;
      }
      const button = e.target.closest('button[data-line], .outline-ticks');
      if (!button || !outline.contains(button)) return;
      const index = e.detail ? outlinePointer(e.clientY) : outlineIndex ?? 0;
      const line = button === outlineTicks ? outlineRows[index]?.line : Number(button.dataset.line);
      outlinePeek(null);
      stop();
      const block = container.querySelector(`.blk[data-start="${line}"]`);
      if (!block) return;
      block.tabIndex = -1;
      block.focus({ preventScroll: true });
      if (!outlineWide) outlineDetails.open = false;
      const inset = outlineWide ? 0 : outlineDetails.querySelector('summary').getBoundingClientRect().height + 8;
      root.scrollTo({ top: Math.max(0, root.scrollY + block.getBoundingClientRect().top - outlineTop() - inset), behavior: root.matchMedia('(prefers-reduced-motion: reduce)').matches ? 'instant' : 'smooth' });
      outlineScroll();
    }

    const outlineResize = outline && root.ResizeObserver ? new ResizeObserver((entries) => {
      const wide = entries[0].contentRect.width >= OUTLINE_WIDE;
      if (wide !== outlineWide) {
        outlineWide = wide;
        closeOutline();
        outlinePeek(null);
      }
      outlineScroll();
    }) : null;
    if (outlineNav) {
      outline.addEventListener('click', outlineGo);
      outlineDetails.addEventListener('pointerdown', outlineDown);
      outlineDetails.addEventListener('pointerenter', outlineEnter);
      outlineDetails.addEventListener('pointerleave', outlineExit);
      outlineTicks.addEventListener('pointermove', outlineHover);
      outlineTicks.addEventListener('pointerleave', outlineLeave);
      outlineTicks.addEventListener('focus', outlineFocus);
      outlineTicks.addEventListener('blur', outlineBlur);
      outlineTicks.addEventListener('keydown', outlineKeydown);
      document.addEventListener('keydown', outlineEscape, true);
      document.addEventListener('pointerdown', outlineOutside, true);
      root.addEventListener('scroll', outlineScroll, { passive: true });
      root.addEventListener('resize', outlineScroll);
      if (outlineResize) outlineResize.observe(outline.closest('.note-layout'));
      outlineDraw();
    }

    function blockAtLine(line) {
      return blocks.findIndex((b) => line >= b.start && line < b.end);
    }

    function fit(area) {
      if (editing && editing.initialHeight !== null) {
        area.style.height = `${editing.initialHeight}px`;
        area.style.overflowY = 'auto';
        return;
      }
      area.style.overflowY = 'hidden';
      area.style.height = 'auto';
      const lineHeight = parseFloat(getComputedStyle(area).lineHeight);
      const height = editing && editing.kind === 'line' ? Math.round(area.scrollHeight / lineHeight) * lineHeight : area.scrollHeight;
      area.style.height = `${height}px`;
      area.scrollTop = 0;
    }

    let editWidth = null;
    const editResize = root.ResizeObserver ? new ResizeObserver((entries) => {
      const width = entries[0].contentRect.width;
      if (width === editWidth) return;
      editWidth = width;
      if (editing) fit(editing.selectionText || editing.area);
    }) : null;
    if (editResize) editResize.observe(container);

    let kept = { caret: null, picked: null, text: null };
    let told = false;

    function liveSelection() {
      if (!editing || editing.selectionText) return null;
      const { area } = editing;
      if (area.selectionStart === area.selectionEnd) return null;
      const base = lineStart(editing.start) + editing.prefix.length;
      return ed.snapshotOf(lines, base + area.selectionStart, base + area.selectionEnd);
    }

    function keep() {
      if (!editing) return;
      const base = lineStart(editing.start) + editing.prefix.length;
      const span = editing.selectionText && selectedSpan();
      kept = { caret: lineOf(span ? span[1] : base + editing.area.selectionEnd), picked: span ? ed.snapshotOf(lines, span[0], span[1]) : liveSelection(), text: lines.join('\n') };
    }

    function tell() {
      slashSeen();
      if (!onSelection) return;
      requestAnimationFrame(() => {
        const has = Boolean(container.isConnected && ((editing && document.activeElement === editing.area && liveSelection()) || selectedSpan()));
        if (has || told) onSelection(has);
        told = has;
      });
    }

    function caretBox(area, at) {
      const style = getComputedStyle(area);
      const mirror = document.createElement('div');
      for (const p of ['boxSizing', 'width', 'paddingTop', 'paddingRight', 'paddingBottom', 'paddingLeft', 'borderTopWidth', 'borderRightWidth', 'borderBottomWidth', 'borderLeftWidth', 'fontFamily', 'fontSize', 'fontWeight', 'fontStyle', 'letterSpacing', 'lineHeight', 'textTransform', 'wordSpacing', 'tabSize', 'textIndent']) mirror.style[p] = style[p];
      Object.assign(mirror.style, { position: 'absolute', visibility: 'hidden', whiteSpace: 'pre-wrap', overflowWrap: 'break-word', top: '0', left: '-9999px' });
      mirror.textContent = area.value.slice(0, at);
      const mark = document.createElement('span');
      mark.textContent = area.value.slice(at, at + 1) || '.';
      mirror.appendChild(mark);
      document.body.appendChild(mirror);
      const box = area.getBoundingClientRect();
      const frame = mirror.getBoundingClientRect();
      const spot = mark.getBoundingClientRect();
      const point = { x: box.left + spot.left - frame.left - area.scrollLeft, y: box.top + spot.bottom - frame.top - area.scrollTop };
      mirror.remove();
      return point;
    }

    function onSelectionChange() {
      if (!container.isConnected) return;
      if (editing && editing.selectionText && !pressed) settle();
      tell();
    }

    document.addEventListener('selectionchange', onSelectionChange);

    function stop() {
      if (!editing) return;
      keep();
      dragOut = null;
      dragStart = null;
      dragAnchor = null;
      sourcePress = null;
      const { area } = editing;
      area.removeEventListener('input', onInput);
      area.removeEventListener('beforeinput', onBeforeInput);
      area.removeEventListener('keydown', onKey);
      area.removeEventListener('blur', onBlur);
      area.removeEventListener('pointerdown', onAreaPress);
      for (const kind of ['select', 'keyup', 'mouseup']) area.removeEventListener(kind, tell);
      editing = null;
      draw();
      tell();
    }

    function edit(index, caret, reveal = true) {
      const block = blocks[index];
      if (!block) return;
      const length = lines.slice(block.start, block.end).join('\n').length;
      const at = caret === 'start' ? 0 : caret === 'end' || caret == null ? length : Math.max(0, Math.min(caret, length));
      open(index, index, at, at, reveal);
    }

    function open(first, last, from, to, reveal = true) {
      if (editing) stop();
      const block = blocks[first];
      const end = blocks[last];
      if (!block || !end) return;
      const el = container.querySelector(`.blk[data-i="${first}"]`);
      if (!el) return;
      for (let i = first + 1; i <= last; i++) {
        const covered = container.querySelector(`.blk[data-i="${i}"]`);
        if (covered) covered.remove();
      }
      const raw = lines.slice(block.start, end.end).join('\n');
      const single = first === last && block.kind === 'line';
      const margin = !single && el.firstElementChild ? getComputedStyle(el.firstElementChild).margin : null;
      const prefix = single && el.querySelector('li') ? ed.listPrefix(raw) : '';
      const shown = single && !prefix ? textLeft(el) : null;
      const height = el.getBoundingClientRect().height;
      const content = single ? el.querySelector('.task > span') || el.querySelector('li, h2, h3, h4, h5, h6, p') : null;
      const initialHeight = content ? content.getBoundingClientRect().height || height : height;
      const area = document.createElement('textarea');
      area.className = 'line-edit';
      if (margin !== null) area.style.margin = margin;
      area.value = raw.slice(prefix.length);
      area.rows = 1;
      area.setAttribute('autocapitalize', 'sentences');
      if (prefix && el.querySelector('li')) {
        const text = el.querySelector('.task > span') || el.querySelector('li');
        text.replaceChildren(area);
      } else {
        el.removeAttribute('style');
        el.replaceChildren(area);
      }
      el.style.minHeight = `${height}px`;
      el.classList.add('editing');
      editing = { index: first, last, start: block.start, end: end.end, originalEnd: end.end, kind: first === last ? block.kind : 'span', prefix, initialHeight, area };
      look(area);
      if (shown !== null && raw.trim()) {
        const from = caretBox(area, ed.rawOffset(raw, '', false)).x;
        if (Math.abs(shown - from) > 0.5) el.style.marginLeft = `${shown - from}px`;
      }
      fit(area);
      area.addEventListener('input', onInput);
      area.addEventListener('beforeinput', onBeforeInput);
      area.addEventListener('keydown', onKey);
      area.addEventListener('blur', onBlur);
      area.addEventListener('pointerdown', onAreaPress);
      for (const kind of ['select', 'keyup', 'mouseup']) area.addEventListener(kind, tell);
      area.focus({ preventScroll: true });
      area.setSelectionRange(Math.max(0, Math.min(from - prefix.length, area.value.length)), Math.max(0, Math.min(to - prefix.length, area.value.length)));
      if (reveal && from === to) area.scrollIntoView({ block: 'nearest' });
    }

    function textLeft(el) {
      const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
      let node;
      while ((node = walker.nextNode())) {
        const at = node.data.search(/[^\s\u200b]/);
        if (at < 0) continue;
        const range = document.createRange();
        range.setStart(node, at);
        range.setEnd(node, at + 1);
        const box = range.getClientRects()[0];
        if (box) return box.left;
      }
      return null;
    }

    function look(area) {
      area.spellcheck = !(editing && ['table', 'code', 'math'].includes(editing.kind)) && !/^\s*[-|:]/.test(area.value);
      const head = editing && editing.kind === 'line' && !editing.prefix && area.value.match(/^\s{0,3}(#{1,6})\s/);
      if (head) area.dataset.head = String(Math.min(head[1].length + 1, 6));
      else delete area.dataset.head;
    }

    function pointFor(at) {
      if (editing && editing.selectionText) {
        const base = lineStart(editing.start) + editing.prefix.length;
        if (at >= base && at <= base + editing.area.value.length) return { node: editing.selectionText.firstChild, offset: at - base };
      }
      if (at <= 0) return { node: container, offset: 0 };
      if (at >= docLength()) return { node: container, offset: blocks.length };
      const index = blockAtLine(lineOf(at));
      const block = blocks[index];
      const blk = block && container.querySelector(`.blk[data-i="${index}"]`);
      if (!blk) return { node: container, offset: at > 0 ? container.childNodes.length : 0 };
      const raw = lines.slice(block.start, block.end).join('\n');
      const inside = at - lineStart(block.start);
      const texts = [];
      const walker = document.createTreeWalker(blk, NodeFilter.SHOW_TEXT);
      let node;
      while ((node = walker.nextNode())) texts.push(node);
      const whole = { node: blk, offset: inside > 0 ? blk.childNodes.length : 0 };
      if (block.kind !== 'line' || !raw.trim() || !texts.length) return whole;
      const total = texts.reduce((n, text) => n + text.data.replace(/\u200b/g, '').length, 0);
      const isItem = ed.listed(lines, block.start);
      let low = 0;
      let high = total;
      while (low < high) {
        const mid = (low + high) >> 1;
        if (ed.rawOffset(raw, 'x'.repeat(mid), isItem) >= inside) high = mid;
        else low = mid + 1;
      }
      let seen = 0;
      for (const text of texts) {
        let offset = 0;
        for (const ch of text.data) {
          if (ch !== '\u200b') {
            if (seen >= low) return { node: text, offset };
            seen += ch.length;
          }
          offset += ch.length;
        }
      }
      const last = texts[texts.length - 1];
      return { node: last, offset: last.data.length };
    }

    function selectSpan(from, to) {
      const selection = root.getSelection ? root.getSelection() : null;
      if (!selection) return;
      const a = pointFor(from);
      const b = pointFor(to);
      selection.setBaseAndExtent(a.node, a.offset, b.node, b.offset);
    }

    function lineStart(line) {
      let at = 0;
      for (let k = 0; k < line && k < lines.length; k++) at += lines[k].length + 1;
      return at;
    }

    function hitsOf(from, to) {
      return [...container.querySelectorAll('.blk')].filter((el) => {
        const block = blocks[Number(el.dataset.i)];
        return block && block.start < to && block.end >= from;
      });
    }

    function lineOf(at) {
      let start = 0;
      for (let k = 0; k < lines.length; k++) {
        if (at <= start + lines[k].length) return k;
        start += lines[k].length + 1;
      }
      return lines.length - 1;
    }

    const docLength = () => lines.join('\n').length;

    function editSpan(from, to) {
      if (from > to) [from, to] = [to, from];
      const first = blockAtLine(lineOf(from));
      const last = blockAtLine(lineOf(to));
      if (first < 0 || last < 0) return;
      const base = lineStart(blocks[first].start);
      open(first, last, from - base, to - base);
    }

    function pointAt(node, offset, side) {
      if (node === container) {
        if (offset >= blocks.length) return docLength();
        if (side === 'start') return lineStart(shifted(blocks[offset].start));
        return offset > 0 ? lineStart(shifted(blocks[offset - 1].start) + blocks[offset - 1].end - blocks[offset - 1].start) - 1 : 0;
      }
      const el = node.nodeType === 1 ? node : node.parentElement;
      const blk = el && el.closest('.blk');
      if (!blk || !container.contains(blk)) return side === 'start' ? 0 : docLength();
      if (editing && editing.selectionText && editing.selectionText.contains(node)) {
        const range = document.createRange();
        range.setStart(editing.selectionText, 0);
        range.setEnd(node, offset);
        return lineStart(editing.start) + editing.prefix.length + Math.min(range.toString().length, editing.area.value.length);
      }
      if (editing && blk.classList.contains('editing')) {
        return lineStart(editing.start) + editing.prefix.length + (side === 'start' ? 0 : editing.area.value.length);
      }
      const block = blocks[Number(blk.dataset.i)];
      const start = shifted(block.start);
      const raw = lines.slice(start, start + block.end - block.start).join('\n');
      let inside = side === 'start' ? 0 : raw.length;
      if (block.kind === 'line' && raw.trim()) {
        const range = document.createRange();
        range.setStart(blk, 0);
        try {
          range.setEnd(node, offset);
          inside = ed.rawOffset(raw, range.toString().replace(/\u200b/g, ''), ed.listed(lines, start));
        } catch (e) {
          inside = side === 'start' ? 0 : raw.length;
        }
      }
      return lineStart(start) + inside;
    }

    function selectedSpan() {
      if (editing && document.activeElement === editing.area) return null;
      const selection = root.getSelection ? root.getSelection() : null;
      if (!selection || selection.isCollapsed || !selection.rangeCount) return null;
      const range = selection.getRangeAt(0);
      if (!container.contains(range.startContainer) && !container.contains(range.endContainer)) return null;
      const from = container.contains(range.startContainer) ? pointAt(range.startContainer, range.startOffset, 'start') : 0;
      const to = container.contains(range.endContainer) ? pointAt(range.endContainer, range.endOffset, 'end') : docLength();
      return from === to ? null : [from, to];
    }

    function onDocumentKey(e) {
      if (!container.isConnected || e.isComposing || e.defaultPrevented) return;
      if (editing && document.activeElement === editing.area) return;
      const focus = document.activeElement;
      if ((!focus || focus === document.body || container.contains(focus)) && (historyKey(e) || styleKey(e))) return;
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'a' && (!focus || focus === document.body || container.contains(focus))) {
        e.preventDefault();
        stop();
        selectSpan(0, docLength());
        return;
      }
      const typing = e.key.length === 1 && !e.metaKey && !e.ctrlKey && !e.altKey;
      const enter = e.key === 'Enter' && !e.metaKey && !e.ctrlKey && !e.altKey;
      if (e.key !== 'Backspace' && e.key !== 'Delete' && !typing && !enter) return;
      if (!replaceSelection(typing ? e.key : enter ? '\n' : '')) return;
      e.preventDefault();
    }

    function styleKey(e) {
      const style = (e.metaKey || e.ctrlKey) && !e.altKey && !e.shiftKey && { b: 'bold', i: 'italic', u: 'underline' }[e.key.toLowerCase()];
      if (!style) return false;
      if (editing && document.activeElement === editing.area) {
        e.preventDefault();
        const { area } = editing;
        if (area.selectionStart === area.selectionEnd) {
          if (['code', 'math'].includes(editing.kind)) return true;
          const pair = ed.stylePair(area.value, area.selectionStart, style);
          area.value = pair.text;
          area.setSelectionRange(pair.at, pair.at);
          onInput();
          return true;
        }
        const next = ed.restyle(area.value, area.selectionStart, area.selectionEnd, style);
        area.value = next.text;
        area.setSelectionRange(next.from, next.to);
        onInput();
        return true;
      }
      const span = selectedSpan();
      if (!span) return false;
      e.preventDefault();
      stop();
      const next = ed.restyle(lines.join('\n'), span[0], span[1], style);
      lines = next.text.split('\n');
      changed();
      draw();
      selectSpan(next.from, next.to);
      return true;
    }

    function replaceSelection(text) {
      const span = selectedSpan();
      if (!span) return false;
      stop();
      const before = lines.join('\n');
      lines = (before.slice(0, span[0]) + text + before.slice(span[1])).split('\n');
      const at = span[0] + text.length;
      ed.renumber(lines, lineOf(at));
      changed();
      draw();
      editSpan(at, at);
      return true;
    }

    function noteFocused() {
      const focus = document.activeElement;
      return !focus || focus === document.body || (container.contains(focus) && !(editing && focus === editing.area));
    }

    function onCopy(e) {
      if (!container.isConnected || !noteFocused()) return;
      const span = selectedSpan();
      if (!span || !e.clipboardData) return;
      e.preventDefault();
      e.clipboardData.setData('text/plain', lines.join('\n').slice(span[0], span[1]));
      if (e.type === 'cut') replaceSelection('');
    }

    function onTextPaste(e) {
      if (!container.isConnected || e.defaultPrevented || !noteFocused()) return;
      const text = e.clipboardData && e.clipboardData.getData('text/plain');
      if (!text || pictures(e.clipboardData.files).length || !selectedSpan()) return;
      e.preventDefault();
      replaceSelection(text.replace(/\r\n?/g, '\n'));
    }

    document.addEventListener('copy', onCopy);
    document.addEventListener('cut', onCopy);
    document.addEventListener('paste', onTextPaste);

    document.addEventListener('keydown', onDocumentKey);

    function editLine(line, caret, reveal = true) {
      const index = blockAtLine(line);
      if (index >= 0) edit(index, caret, reveal);
    }

    const lineIcon = (d) => `<svg viewBox="0 0 24 24" aria-hidden="true">${d}</svg>`;
    const SLASH_ICON = {
      Heading: lineIcon('<path d="M5 5v14M13 5v14M5 12h8M17 11l2-1.5V19"/>'),
      Subheading: lineIcon('<path d="M4 5v14M12 5v14M4 12h8M15.5 11a2 2 0 1 1 3.5 1.4L15.5 19H20"/>'),
      'Small heading': lineIcon('<path d="M4 5v14M12 5v14M4 12h8M15.5 10.5h4l-2.5 3a2.3 2.3 0 1 1-1.8 3.8"/>'),
      Bullet: lineIcon('<circle cx="5.5" cy="7" r="1"/><circle cx="5.5" cy="17" r="1"/><path d="M10 7h10M10 17h10"/>'),
      'Numbered list': lineIcon('<path d="M10 7h10M10 17h10M4.5 5.5l1.5-1v5M4.5 15.2a1.4 1.4 0 1 1 2.2 1.4L4.5 19.5h3"/>'),
      Checkbox: lineIcon('<rect x="4.5" y="4.5" width="15" height="15" rx="3"/><path d="M8.5 12.3l2.4 2.4 4.6-5"/>'),
      Quote: lineIcon('<path d="M6 5v14M10 8h9M10 12h9M10 16h6"/>'),
      'Warning box': md.noteIcon('warning', true),
      'Note box': md.noteIcon('info', true),
      'Tip box': md.noteIcon('idea', true),
      'Code block': lineIcon('<path d="M8.5 7.5L4 12l4.5 4.5M15.5 7.5L20 12l-4.5 4.5"/>'),
      Divider: lineIcon('<path d="M3.5 12h17M8 7h8M8 17h8"/>'),
    };
    const SLASH_ROWS = [...ed.SLASH.map((row) => ({ ...row, icon: SLASH_ICON[row.text] })), ...Object.entries(md.NOTE_ICONS).map(([name, [text]]) => ({ text: `${text} icon`, mark: `:${name}:`, icon: md.noteIcon(name, true), name }))];
    let slash = null;
    let slashGone = null;

    function slashSeen() {
      const area = editing && editing.kind === 'line' && document.activeElement === editing.area ? editing.area : null;
      const typed = area && area.selectionStart === area.selectionEnd && area.value.slice(0, area.selectionStart).match(/(?:^|\s)\/([a-z]*)$/i);
      const word = typed ? typed[1].toLowerCase() : '';
      const rows = typed ? SLASH_ROWS.filter((row) => !word || row.name?.startsWith(word) || row.text.toLowerCase().split(' ').some((part) => part.startsWith(word))) : [];
      const at = typed ? area.selectionStart - word.length - 1 : -1;
      if (slashGone && (slashGone.area !== area || slashGone.at !== at)) slashGone = null;
      if (!rows.length || slashGone) return slashClose();
      if (slash && slash.area === area && slash.at === at && slash.word === word) return;
      if (!slash || slash.area !== area) {
        slashClose();
        const box = document.createElement('div');
        box.className = 'popup-menu slash-menu';
        box.id = 'slash-menu';
        box.setAttribute('role', 'listbox');
        box.setAttribute('aria-label', 'Insert');
        box.addEventListener('mousedown', (e) => e.preventDefault());
        box.addEventListener('click', (e) => {
          const row = e.target.closest('[data-row]');
          if (row) slashPick(Number(row.dataset.row));
        });
        document.body.appendChild(box);
        slash = { area, box };
        area.setAttribute('aria-controls', 'slash-menu');
        addEventListener('scroll', slashPlace, true);
      }
      Object.assign(slash, { at, word, rows });
      slash.box.innerHTML = rows.map((row, i) => `${i && row.name && !rows[i - 1].name ? '<hr aria-hidden="true">' : ''}<button type="button" tabindex="-1" role="option" id="slash-${i}" data-row="${i}">${row.icon}<span class="grow">${md.escape(row.text)}</span>${row.name ? `<kbd>${row.mark}</kbd>` : ''}</button>`).join('');
      const spot = slashPlace();
      slashMove(0);
      if (!slash.box.dataset.shown) {
        slash.box.dataset.shown = '1';
        window.leoMotion?.unfold(slash.box, spot);
      }
    }

    function slashPlace() {
      const { area, box, at } = slash;
      const spot = caretBox(area, at);
      const high = parseFloat(getComputedStyle(area).lineHeight) || 24;
      box.style.left = `${Math.max(8, Math.min(spot.x - 12, root.innerWidth - box.offsetWidth - 8))}px`;
      box.style.top = `${spot.y + 6 + box.offsetHeight > root.innerHeight - 8 ? Math.max(8, spot.y - high - box.offsetHeight - 6) : spot.y + 6}px`;
      return spot;
    }

    function slashMove(i) {
      slash.active = i;
      for (const row of slash.box.querySelectorAll('[data-row]')) row.setAttribute('aria-selected', String(Number(row.dataset.row) === i));
      slash.area.setAttribute('aria-activedescendant', `slash-${i}`);
      slash.box.querySelector(`#slash-${i}`)?.scrollIntoView({ block: 'nearest' });
    }

    function slashClose() {
      if (!slash) return;
      const { area, box } = slash;
      slash = null;
      removeEventListener('scroll', slashPlace, true);
      area.removeAttribute('aria-activedescendant');
      area.removeAttribute('aria-controls');
      for (const named of [box, ...box.querySelectorAll('[id]')]) named.removeAttribute('id');
      box.className = 'popup-menu-gone slash-menu';
      box.setAttribute('aria-hidden', 'true');
      box.inert = true;
      if (window.leoMotion) window.leoMotion.fold(box, () => box.remove());
      else box.remove();
    }

    function slashPick(i) {
      const row = slash && slash.rows[i];
      if (!row || !editing) return;
      const { area, start, prefix } = editing;
      const value = prefix + area.value;
      const caret = prefix.length + area.selectionStart;
      const from = value.lastIndexOf('\n', caret - 1) + 1;
      const line = start + value.slice(0, from).split('\n').length - 1;
      const done = ed.slash(lines[line], lines.slice(0, line), prefix.length + slash.at - from, caret - from, row.mark);
      slashClose();
      lines.splice(line, 1, ...done.lines);
      ed.renumber(lines, line + done.line, line + done.line);
      changed();
      stop();
      const target = line + done.line;
      const index = blockAtLine(target);
      if (index >= 0) edit(index, lineStart(target) - lineStart(blocks[index].start) + done.caret);
    }

    function onInput(e) {
      if (told) tell();
      const { area, start } = editing;
      editing.initialHeight = null;
      area.closest('.blk').style.minHeight = '';
      const next = (editing.prefix + area.value).split('\n');
      lines.splice(start, editing.end - start, ...next);
      editing.end = start + next.length;
      look(area);
      fit(area);
      changed();
      if (!e?.isComposing) slashSeen();
      const marker = ed.listPrefix(editing.prefix + area.value, lines.slice(0, start));
      if (editing.kind === 'line' && !e?.isComposing && marker.length > editing.prefix.length && /[^\S\n]$/.test(marker)) {
        const caret = lineStart(start) + editing.prefix.length + area.selectionStart;
        const line = lineOf(caret);
        const inside = caret - lineStart(line);
        stop();
        editLine(line, inside);
      }
    }

    function singleRow(area) {
      const style = getComputedStyle(area);
      const lineHeight = parseFloat(style.lineHeight) || 24;
      return area.scrollHeight <= lineHeight * 1.6;
    }

    function enterLine() {
      const { area, start, prefix } = editing;
      const value = prefix + area.value;
      const before = value.slice(0, prefix.length + area.selectionStart).split('\n');
      const after = value.slice(prefix.length + area.selectionEnd);
      const above = lines.slice(0, start).concat(before.slice(0, -1));
      const next = ed.continueLine(before[before.length - 1], above);
      const span = editing.end - start;
      if (next.exit && !after.trim()) {
        before[before.length - 1] = next.back;
        lines.splice(start, span, ...before);
        ed.closeList(lines, start + before.length - 1);
        changed();
        stop();
        editLine(start + before.length - 1, 'end');
        return;
      }
      lines.splice(start, span, ...before, ...(next.prefix + after).split('\n'));
      ed.renumber(lines, start + before.length);
      changed();
      stop();
      editLine(start + before.length, ed.itemHead(lines[start + before.length], lines.slice(0, start + before.length)).length);
    }

    function onBeforeInput(e) {
      if (!editing) return;
      const soft = editing.softBreak;
      editing.softBreak = false;
      if (!e.cancelable || e.isComposing || soft || editing.kind !== 'line' || !(['insertLineBreak', 'insertParagraph'].includes(e.inputType) || e.inputType === 'insertText' && e.data === '\n')) return;
      e.preventDefault();
      enterLine();
    }

    function onKey(e) {
      if (e.isComposing || e.keyCode === 229) return;
      if (slash && !e.metaKey && !e.ctrlKey && !e.altKey) {
        if ((e.key === 'ArrowDown' || e.key === 'ArrowUp') && !e.shiftKey) {
          e.preventDefault();
          slashMove((slash.active + (e.key === 'ArrowDown' ? 1 : slash.rows.length - 1)) % slash.rows.length);
          return;
        }
        if ((e.key === 'Enter' || e.key === 'Tab') && !e.shiftKey) {
          e.preventDefault();
          slashPick(slash.active);
          return;
        }
        if (e.key === 'Escape') {
          e.preventDefault();
          e.stopPropagation();
          slashGone = { area: slash.area, at: slash.at };
          slashClose();
          return;
        }
      }
      editing.softBreak = e.key === 'Enter' && e.shiftKey;
      if (historyKey(e) || styleKey(e)) return;
      const { area, start, kind } = editing;
      const prefix = editing.prefix;
      const pos = prefix.length + area.selectionStart;
      const collapsed = area.selectionStart === area.selectionEnd;
      const value = prefix + area.value;

      if (e.key === 'Escape') {
        e.preventDefault();
        area.blur();
        return;
      }

      if (kind === 'line' && e.key === 'Enter' && !e.shiftKey) {
        e.preventDefault();
        enterLine();
        return;
      }

      if (kind === 'line' && e.key === 'Backspace' && collapsed && pos && pos === ed.itemHead(value, lines.slice(0, start)).length) {
        e.preventDefault();
        lines.splice(start, editing.end - start, ...ed.stepBack(value, lines.slice(0, start)).split('\n'));
        ed.closeList(lines, start);
        changed();
        stop();
        editLine(start, ed.itemHead(lines[start], lines.slice(0, start)).length);
        return;
      }

      if (kind === 'line' && e.key === 'Backspace' && collapsed && pos === 0 && start > 0) {
        const previous = blocks[blockAtLine(start - 1)];
        if (previous && previous.kind === 'line') {
          e.preventDefault();
          const above = lines[start - 1];
          lines.splice(start - 1, 1 + editing.end - start, ...(above + value).split('\n'));
          ed.renumber(lines, start - 1);
          changed();
          stop();
          editLine(start - 1, above.length);
        }
        return;
      }

      if (kind === 'table' && e.key === 'Tab') {
        e.preventDefault();
        const cell = ed.nextCell(area.value, area.selectionEnd, e.shiftKey);
        if (cell) {
          area.setSelectionRange(cell.from, cell.to);
          return;
        }
        const row = ed.addRow(area.value);
        area.value = row.text;
        area.setSelectionRange(row.at, row.at);
        onInput();
        return;
      }

      if (kind === 'line' && e.key === 'Tab') {
        e.preventDefault();
        const above = lines.slice(0, start);
        const next = e.shiftKey ? ed.outdent(value, above) : ed.indent(value, above);
        if (next === value) return;
        lines.splice(start, editing.end - start, ...next.split('\n'));
        ed.renumber(lines, start, start);
        const shift = lines[start].length - value.split('\n')[0].length;
        changed();
        stop();
        editLine(start, Math.max(0, pos + shift));
        return;
      }

      if ((e.metaKey || e.ctrlKey) && !e.shiftKey && !e.altKey && e.key.toLowerCase() === 'a') {
        e.preventDefault();
        stop();
        selectSpan(0, docLength());
        return;
      }

      if (MAC && kind === 'line' && (e.key === 'End' || e.key === 'Home') && !e.metaKey && !e.ctrlKey && !e.altKey) {
        e.preventDefault();
        const to = e.key === 'End' ? area.value.length : 0;
        area.setSelectionRange(e.shiftKey ? Math.min(to, area.selectionStart) : to, e.shiftKey ? Math.max(to, area.selectionEnd) : to);
        return;
      }

      if (e.key === 'ArrowUp' && collapsed && (pos === prefix.length || (kind === 'line' && singleRow(area))) && start > 0) {
        e.preventDefault();
        stop();
        editLine(start - 1, 'end');
        return;
      }

      if (e.key === 'ArrowDown' && collapsed && (pos === value.length || (kind === 'line' && singleRow(area))) && editing.end < lines.length) {
        e.preventDefault();
        const below = editing.end;
        stop();
        editLine(below, 'start');
      }
    }

    function settle() {
      if (editing && document.activeElement !== editing.area && !(editing.selectionText && selectedSpan())) stop();
    }

    function onBlur() {
      setTimeout(() => {
        if (!pressed) settle();
      }, 0);
    }

    function caretIn(el, x, y) {
      let node = null;
      let offset = 0;
      if (document.caretPositionFromPoint) {
        const pos = document.caretPositionFromPoint(x, y);
        if (pos) {
          node = pos.offsetNode;
          offset = pos.offset;
        }
      } else if (document.caretRangeFromPoint) {
        const range = document.caretRangeFromPoint(x, y);
        if (range) {
          node = range.startContainer;
          offset = range.startOffset;
        }
      }
      if (!node || !el.contains(node)) return null;
      const range = document.createRange();
      range.setStart(el, 0);
      try {
        range.setEnd(node, offset);
      } catch (e) {
        return null;
      }
      return range.toString().replace(/\u200b/g, '');
    }

    let pressed = false;
    let release;

    let extended = false;

    let dragOut = null;
    let dragStart = null;
    let dragAnchor = null;
    let sourcePress = null;
    let downAt = null;
    const DRAG_CLICK_DISTANCE = 4;

    function onAreaPress(e) {
      dragOut = e.button === 0 && !e.shiftKey && e.pointerType === 'mouse' ? e.currentTarget : null;
      dragStart = null;
    }

    function caretAt(x, y) {
      if (document.caretPositionFromPoint) {
        const pos = document.caretPositionFromPoint(x, y);
        if (pos && container.contains(pos.offsetNode)) return { node: pos.offsetNode, offset: pos.offset };
      } else if (document.caretRangeFromPoint) {
        const range = document.caretRangeFromPoint(x, y);
        if (range && container.contains(range.startContainer)) return { node: range.startContainer, offset: range.startOffset };
      }
      const box = container.getBoundingClientRect();
      if (y < box.top) return { node: container, offset: 0 };
      if (y > box.bottom) return { node: container, offset: container.childNodes.length };
      return null;
    }

    function onDragMove(e) {
      if (dragAnchor) {
        if (!(e.buttons & 1)) {
          dragAnchor = null;
          return;
        }
        e.preventDefault();
        const to = caretAt(e.clientX, e.clientY);
        const selection = root.getSelection();
        if (to && selection) selection.setBaseAndExtent(dragAnchor.node, dragAnchor.offset, to.node, to.offset);
        return;
      }
      if (!dragOut) return;
      if (!(e.buttons & 1) || !editing || editing.area !== dragOut) {
        dragOut = null;
        return;
      }
      const area = dragOut;
      if (dragStart === null) dragStart = area.selectionStart;
      const box = area.getBoundingClientRect();
      if (e.clientY >= box.top && e.clientY <= box.bottom) return;
      dragOut = null;
      const anchor = lineStart(editing.start) + editing.prefix.length + dragStart;
      const text = document.createElement('div');
      text.className = area.className;
      text.style.cssText = area.style.cssText;
      text.style.whiteSpace = 'pre-wrap';
      text.style.overflowWrap = 'break-word';
      if (area.dataset.head) text.dataset.head = area.dataset.head;
      text.textContent = area.value || '\u200b';
      const top = area.scrollTop, left = area.scrollLeft;
      editing.selectionText = text;
      area.replaceWith(text);
      text.scrollTop = top;
      text.scrollLeft = left;
      dragAnchor = pointFor(anchor);
      extended = true;
      onDragMove(e);
    }

    function onDragEnd() {
      dragOut = null;
      dragStart = null;
      dragAnchor = null;
    }

    document.addEventListener('pointermove', onDragMove);
    document.addEventListener('pointerup', onDragEnd);
    document.addEventListener('pointercancel', onDragEnd);

    container.addEventListener('pointerdown', (e) => {
      downAt = { x: e.clientX, y: e.clientY };
      pressed = true;
      extended = false;
      clearTimeout(release);
      sourcePress = e.button === 0 && !e.shiftKey && editing && editing.selectionText && editing.selectionText.contains(e.target) ? { x: e.clientX, y: e.clientY } : null;
      if (!e.shiftKey || !editing || e.target.closest('.editing')) return;
      const blk = e.target.closest('.blk');
      if (!blk) return;
      const block = blocks[Number(blk.dataset.i)];
      const start = shifted(block.start);
      const raw = lines.slice(start, start + block.end - block.start).join('\n');
      const prefix = block.kind === 'line' ? caretIn(blk, e.clientX, e.clientY) : null;
      const anchor = lineStart(editing.start) + editing.prefix.length + editing.area.selectionStart;
      const below = start >= editing.end;
      const inside = prefix !== null && raw.trim() ? ed.rawOffset(raw, prefix, ed.listed(lines, start)) : below ? raw.length : 0;
      e.preventDefault();
      extended = true;
      const target = lineStart(start) + inside;
      stop();
      selectSpan(anchor, target);
    });

    container.addEventListener('pointercancel', () => {
      pressed = false;
      settle();
    });

    function onPointerUp() {
      if (!pressed) return;
      clearTimeout(release);
      release = setTimeout(() => {
        pressed = false;
        settle();
      }, 400);
    }

    document.addEventListener('pointerup', onPointerUp);

    function shifted(line) {
      if (editing && line >= editing.originalEnd) return line + editing.end - editing.originalEnd;
      return line;
    }

    function boxFor(e) {
      const direct = e.target.closest('input[data-box]');
      if (direct) return direct;
      if (!downAt || Math.hypot(e.clientX - downAt.x, e.clientY - downAt.y) >= DRAG_CLICK_DISTANCE) return null;
      const item = e.target.closest('.task-item');
      const input = item && item.querySelector('input[data-box]');
      const text = item && item.querySelector('label.task > span');
      if (!input || !text) return null;
      return e.clientX < text.getBoundingClientRect().left ? input : null;
    }

    let framed = null;

    function unframe() {
      if (!framed) return;
      framed.frame.remove();
      framed.img.classList.remove('picked');
      framed = null;
    }

    function innerWidth(img) {
      return img.getBoundingClientRect().width;
    }

    function placeFrame() {
      if (!framed) return;
      const box = container.getBoundingClientRect();
      const r = framed.img.getBoundingClientRect();
      Object.assign(framed.frame.style, { left: `${r.left - box.left}px`, top: `${r.top - box.top}px`, width: `${r.width}px`, height: `${r.height}px` });
      framed.frame.querySelector('.pic-size').textContent = `${Math.round(innerWidth(framed.img))} px`;
    }

    function pictureAt(img) {
      const blk = img.closest('.blk');
      if (!blk) return null;
      const block = blocks[Number(blk.dataset.i)];
      if (!block) return null;
      return { start: shifted(block.start), length: block.end - block.start, index: [...blk.querySelectorAll('img.note-img')].indexOf(img) };
    }

    function frame(img) {
      unframe();
      const frameEl = document.createElement('div');
      frameEl.className = 'pic-frame';
      frameEl.innerHTML = '<span class="pic-size"></span><button type="button" class="pic-handle" aria-label="Resize picture: drag, or use the arrow keys" title="Drag to resize · double-click to reset"></button>';
      container.appendChild(frameEl);
      img.classList.add('picked');
      framed = { img, frame: frameEl };
      placeFrame();
      const handle = frameEl.querySelector('.pic-handle');
      handle.addEventListener('pointerdown', (e) => {
        e.preventDefault();
        e.stopPropagation();
        handle.setPointerCapture(e.pointerId);
        const startX = e.clientX;
        const startWidth = innerWidth(img);
        const most = Math.max(48, (img.closest('.blk') || container).clientWidth);
        let width = startWidth;
        const move = (m) => {
          if (!(m.buttons & 1)) return up();
          width = Math.round(Math.min(most, Math.max(48, startWidth + m.clientX - startX)));
          img.style.width = `${width}px`;
          placeFrame();
        };
        let done = false;
        const up = () => {
          if (done) return;
          done = true;
          handle.removeEventListener('pointermove', move);
          handle.removeEventListener('pointerup', up);
          handle.removeEventListener('pointercancel', up);
          handle.removeEventListener('lostpointercapture', up);
          if (handle.hasPointerCapture(e.pointerId)) handle.releasePointerCapture(e.pointerId);
          if (Math.abs(width - startWidth) >= 2) resizePicture(img, width);
        };
        handle.addEventListener('pointermove', move);
        handle.addEventListener('pointerup', up);
        handle.addEventListener('pointercancel', up);
        handle.addEventListener('lostpointercapture', up);
      });
      handle.addEventListener('dblclick', (e) => {
        e.preventDefault();
        e.stopPropagation();
        resizePicture(img, 0);
      });
      handle.addEventListener('keydown', (e) => {
        if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') return;
        e.preventDefault();
        const now = innerWidth(img);
        resizePicture(img, Math.max(48, Math.round(now + (e.key === 'ArrowRight' ? 1 : -1) * (e.shiftKey ? 100 : 20))));
      });
    }

    function resizePicture(img, width) {
      const at = pictureAt(img);
      if (!at || at.index < 0) return;
      if (editing) stop();
      const before = lines.slice(at.start, at.start + at.length).join('\n');
      const after = ed.setPictureWidth(before, at.index, width);
      if (after === before) return;
      lines.splice(at.start, at.length, ...after.split('\n'));
      changed();
      draw();
      const blk = container.querySelector(`.blk[data-start="${at.start}"]`);
      const again = blk && blk.querySelectorAll('img.note-img')[at.index];
      if (!again) return;
      const show = () => {
        frame(again);
        framed.frame.querySelector('.pic-handle').focus({ preventScroll: true });
      };
      if (again.complete) show();
      else again.addEventListener('load', show, { once: true });
    }

    container.addEventListener('dblclick', (e) => {
      const img = e.target.closest('img.note-img');
      if (!img || img.closest('.editing')) return;
      const blk = img.closest('.blk');
      unframe();
      if (blk) editLine(shifted(Number(blk.dataset.start)), 'end', false);
    });

    function onFrameKey(e) {
      if (e.key !== 'Escape' || !framed || !framed.frame.isConnected) return;
      e.preventDefault();
      e.stopPropagation();
      unframe();
    }
    document.addEventListener('keydown', onFrameKey, true);
    root.addEventListener('resize', placeFrame);

    container.addEventListener('click', (e) => {
      pressed = false;
      clearTimeout(release);
      if (extended) {
        extended = false;
        return;
      }
      if (e.target.closest('.pic-frame')) return;
      const pic = e.target.closest('img.note-img');
      if (pic && !pic.closest('.editing') && !e.shiftKey) {
        e.preventDefault();
        frame(pic);
        return;
      }
      unframe();
      const resume = sourcePress && Math.hypot(e.clientX - sourcePress.x, e.clientY - sourcePress.y) < DRAG_CLICK_DISTANCE;
      sourcePress = null;
      if (resume && editing && editing.selectionText && editing.selectionText.contains(e.target)) {
        e.preventDefault();
        const { area, selectionText } = editing;
        const prefix = caretIn(selectionText, e.clientX, e.clientY);
        const top = selectionText.scrollTop, left = selectionText.scrollLeft;
        area.style.cssText = selectionText.style.cssText;
        selectionText.replaceWith(area);
        delete editing.selectionText;
        area.focus({ preventScroll: true });
        const caret = prefix === null ? area.value.length : prefix.length;
        area.setSelectionRange(caret, caret);
        area.scrollTop = top;
        area.scrollLeft = left;
        return;
      }
      const span = !e.target.closest('input[data-box], a') && selectedSpan();
      if (span) {
        e.preventDefault();
        if (editing && !editing.selectionText) {
          stop();
          selectSpan(span[0], span[1]);
        }
        return;
      }
      const box = boxFor(e);
      if (box) {
        if (e.target !== box) e.preventDefault();
        const line = shifted(Number(box.closest('.blk').dataset.start));
        const top = box.getBoundingClientRect().top;
        const wasEditing = Boolean(editing);
        stop();
        if (Number.isInteger(line) && line >= 0 && line < lines.length && ed.isBox(lines[line])) {
          lines[line] = ed.toggleBox(lines[line]);
          changed();
          const input = wasEditing ? container.querySelector(`.blk[data-start="${line}"] input[data-box]`) : box;
          if (input) {
            const checked = ed.itemHead(lines[line]).includes('[x]');
            input.checked = checked;
            input.defaultChecked = checked;
            input.closest('label.task').classList.toggle('done', checked);
            input.focus({ preventScroll: true });
            root.scrollBy(0, input.getBoundingClientRect().top - top);
          }
        }
        return;
      }
      if (e.target.closest('summary')) return;
      if (e.target.closest('label.task') && !e.target.closest('textarea')) e.preventDefault();
      const blk = e.target.closest('.blk');
      const tail = !blk && e.target.closest('.doc-tail');
      if (e.target.closest('a') || (!blk && !tail) || (blk && blk.classList.contains('editing'))) {
        settle();
        return;
      }
      if (tail) {
        stop();
        if (lines[lines.length - 1].trim()) {
          lines.push('');
          changed();
          draw();
        }
        editLine(lines.length - 1, 'end', false);
        return;
      }
      const lone = !blk.textContent.trim() && blk.querySelector('img.note-img');
      if (lone) {
        frame(lone);
        return;
      }
      const block = blocks[Number(blk.dataset.i)];
      const prefix = block && block.kind === 'line' ? caretIn(blk, e.clientX, e.clientY) : null;
      const line = shifted(Number(blk.dataset.start));
      const cell = block && block.kind === 'table' ? e.target.closest('td, th') : null;
      stop();
      const raw = lines[line] || '';
      const caret = cell ? ed.cellEnd(lines.slice(line, line + block.end - block.start).join('\n'), cell.parentNode.rowIndex, cell.cellIndex) : prefix !== null && raw.trim() ? ed.rawOffset(raw, prefix, ed.listed(lines, line)) : 'end';
      editLine(line, caret, false);
    });

    draw();

    function insert(text) {
      if (editing && editing.selectionText) stop();
      if (editing) {
        const { area } = editing;
        const at = area.selectionStart;
        const before = area.value.slice(0, at);
        const lead = before && !before.endsWith('\n') ? '\n' : '';
        area.setRangeText(`${lead}${text}\n`, at, area.selectionEnd, 'end');
        onInput();
        return;
      }
      const last = lines[lines.length - 1];
      if (last.trim()) lines.push('');
      if (lines.length === 1 && !lines[0].trim()) lines[0] = text;
      else lines.push(text);
      changed();
      draw();
    }

    const pictures = (list) => [...(list || [])].filter((f) => /^image\//.test(f.type));
    function onPaste(e) {
      const found = pictures(e.clipboardData && e.clipboardData.files);
      if (!onPictures || !found.length) return;
      e.preventDefault();
      onPictures(found);
    }
    function onDrop(e) {
      const dropped = [...((e.dataTransfer && e.dataTransfer.files) || [])];
      const found = pictures(dropped);
      // Only pictures go straight into the note; a drop with any other file
      // goes on to the page, which offers to add all of them to this note.
      if (!onPictures || !found.length || found.length !== dropped.length) return;
      e.preventDefault();
      e.stopPropagation();
      onPictures(found);
    }
    container.addEventListener('paste', onPaste);
    container.addEventListener('drop', onDrop);

    return {
      destroy: () => {
        stop();
        clearTimeout(outlineTimer);
        outlineMotion?.cancel();
        outlinePreviewMotion?.cancel();
        cancelAnimationFrame(outlineFrame);
        if (outlineResize) outlineResize.disconnect();
        if (editResize) editResize.disconnect();
        if (outlineNav) {
          outline.removeEventListener('click', outlineGo);
          outlineDetails.removeEventListener('pointerdown', outlineDown);
          outlineDetails.removeEventListener('pointerenter', outlineEnter);
          outlineDetails.removeEventListener('pointerleave', outlineExit);
          outlineTicks.removeEventListener('pointermove', outlineHover);
          outlineTicks.removeEventListener('pointerleave', outlineLeave);
          outlineTicks.removeEventListener('focus', outlineFocus);
          outlineTicks.removeEventListener('blur', outlineBlur);
          outlineTicks.removeEventListener('keydown', outlineKeydown);
          document.removeEventListener('keydown', outlineEscape, true);
          document.removeEventListener('pointerdown', outlineOutside, true);
          outlineTicks.remove();
          outlinePreview.remove();
          outlineUpArrow.remove();
          outlineDownArrow.remove();
        }
        root.removeEventListener('scroll', outlineScroll);
        root.removeEventListener('resize', outlineScroll);
        document.removeEventListener('selectionchange', onSelectionChange);
        clearTimeout(release);
        document.removeEventListener('pointerup', onPointerUp);
        document.removeEventListener('keydown', onDocumentKey);
        document.removeEventListener('copy', onCopy);
        document.removeEventListener('cut', onCopy);
        document.removeEventListener('paste', onTextPaste);
        document.removeEventListener('pointermove', onDragMove);
        document.removeEventListener('pointerup', onDragEnd);
        document.removeEventListener('pointercancel', onDragEnd);
        document.removeEventListener('keydown', onFrameKey, true);
        root.removeEventListener('resize', placeFrame);
        unframe();
        container.removeEventListener('paste', onPaste);
        container.removeEventListener('drop', onDrop);
      },
      insert,
      source: () => lines.join('\n'),
      stop,
      isEditing: () => editing !== null,
      selection() {
        const live = editing && liveSelection();
        if (live) return live;
        const span = selectedSpan();
        if (span) return ed.snapshotOf(lines, span[0], span[1]);
        return kept.picked && kept.text === lines.join('\n') ? kept.picked : null;
      },
      selectionBox() {
        if (editing && document.activeElement === editing.area && editing.area.selectionStart !== editing.area.selectionEnd) return caretBox(editing.area, editing.area.selectionEnd);
        const picked = root.getSelection ? root.getSelection() : null;
        if (!picked || picked.isCollapsed || !picked.rangeCount || !selectedSpan()) return null;
        const box = picked.getRangeAt(0).getBoundingClientRect();
        return { x: box.right, y: box.bottom };
      },
      caretLine() {
        if (editing) return lineOf(lineStart(editing.start) + editing.prefix.length + editing.area.selectionEnd);
        return kept.text === lines.join('\n') ? kept.caret : null;
      },
      glow(from, to) {
        for (const el of container.querySelectorAll('.blk-glow')) el.classList.remove('blk-glow');
        if (!from) return;
        for (const el of hitsOf(from, to)) el.classList.add('blk-glow');
      },
      highlight(from, to) {
        const hits = hitsOf(from, to);
        if (!hits.length) return false;
        for (const el of container.querySelectorAll('.blk-cited')) el.classList.remove('blk-cited');
        for (const el of hits) el.classList.add('blk-cited');
        hits[0].scrollIntoView({ block: 'center', behavior: 'smooth' });
        setTimeout(() => hits.forEach((el) => el.classList.remove('blk-cited')), CITED_FOR);
        return true;
      },
      spanRects(from, to) {
        if (editing && !editing.selectionText) {
          const base = lineStart(editing.start) + editing.prefix.length;
          const { area } = editing;
          if (from >= base && from <= base + area.value.length) {
            const a = caretBox(area, from - base);
            const b = caretBox(area, Math.min(to, base + area.value.length) - base);
            const tall = parseFloat(getComputedStyle(area).lineHeight) || 24;
            const box = area.getBoundingClientRect();
            if (a.y === b.y) return [{ left: a.x, right: b.x, top: a.y - tall, bottom: a.y }];
            return [{ left: a.x, right: box.right, top: a.y - tall, bottom: a.y }, { left: box.left, right: b.x, top: b.y - tall, bottom: b.y }];
          }
        }
        const a = pointFor(from);
        const b = pointFor(to);
        const range = document.createRange();
        try {
          range.setStart(a.node, a.offset);
          range.setEnd(b.node, b.offset);
        } catch (e) {
          return [];
        }
        const boxes = () => {
          const root = range.commonAncestorContainer;
          const walker = document.createTreeWalker(root.nodeType === Node.TEXT_NODE ? root.parentNode : root, NodeFilter.SHOW_TEXT);
          const piece = document.createRange();
          const found = [];
          const begin = range.startContainer;
          walker.currentNode = begin;
          for (let node = begin.nodeType === Node.TEXT_NODE ? begin : walker.nextNode(); node; node = walker.nextNode()) {
            if (range.comparePoint(node, 0) > 0) break;
            if (!range.intersectsNode(node)) continue;
            piece.selectNodeContents(node);
            if (node === range.startContainer) piece.setStart(node, range.startOffset);
            if (node === range.endContainer) piece.setEnd(node, range.endOffset);
            for (const box of piece.getClientRects()) if (box.width > 0) found.push(box);
            if (node === range.endContainer) break;
          }
          const lines = [];
          for (const { left, right, top, bottom } of found.sort((a, b) => a.top - b.top)) {
            const line = lines.find((l) => (top + bottom) / 2 > l.top && (top + bottom) / 2 < l.bottom);
            if (line) Object.assign(line, { left: Math.min(line.left, left), right: Math.max(line.right, right), top: Math.min(line.top, top), bottom: Math.max(line.bottom, bottom) });
            else lines.push({ left, right, top, bottom });
          }
          return lines.slice(0, 24);
        };
        const found = boxes();
        if (found.length) return found;
        const index = blockAtLine(lineOf(from));
        const blk = index < 0 ? null : container.querySelector(`.blk[data-i="${index}"]`);
        if (!blk) return [];
        const source = lines.join('\n');
        const wanted = source.slice(from, to).trim();
        const texts = [];
        const walker = document.createTreeWalker(blk, NodeFilter.SHOW_TEXT);
        for (let node = walker.nextNode(); node; node = walker.nextNode()) texts.push(node);
        const shown = texts.map((node) => node.data).join('');
        let earlier = 0;
        for (let seen = wanted ? source.indexOf(wanted, lineStart(blocks[index].start)) : -1; seen >= 0 && seen < from; seen = source.indexOf(wanted, seen + 1)) earlier++;
        let at = -1;
        for (let i = 0; wanted && i <= earlier; i++) {
          const next = shown.indexOf(wanted, at + 1);
          if (next < 0) break;
          at = next;
        }
        const spot = (n) => {
          for (const node of texts) {
            if (n <= node.data.length) return [node, n];
            n -= node.data.length;
          }
          return null;
        };
        const start = at >= 0 && spot(at);
        const end = at >= 0 && spot(at + wanted.length);
        if (start && end) {
          range.setStart(...start);
          range.setEnd(...end);
          const inside = boxes();
          if (inside.length) return inside;
        }
        const { left, right, top, bottom } = blk.getBoundingClientRect();
        return right > left ? [{ left, right, top, bottom }] : [];
      },
      blockAt(at) {
        const index = blockAtLine(lineOf(at));
        return index < 0 ? null : container.querySelector(`.blk[data-i="${index}"]`);
      },
      editStart: () => editLine(0, 'start'),
      editEnd: () => editLine(lines.length - 1, 'end'),
    };
  }

  const CITED_FOR = 4000;
  const MAC = /Mac/.test(root.navigator?.platform || '');

  root.leoDoc = { mount };
})(window);
