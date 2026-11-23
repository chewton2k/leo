(function (root) {
  'use strict';

  const { MARKER, MATH_ENV, MATH_THEN, MATH_END, listKind, listable, caption } = root.leoMarkdown || require('./markdown.js');
  const FENCE = /^\s*(```|~~~)/;
  const TABLE_RULE = /^\s*\|?\s*:?-+:?\s*(\|\s*:?-+:?\s*)*\|?\s*$/;
  const QUOTE = /^\s*>/;
  const BOX = new RegExp(String.raw`^(?:${MARKER}) {1,4}\[( |x|X)\](?: |$)`);

  const isBox = (line) => BOX.test(line.trimStart());

  function splitBlocks(lines) {
    const blocks = [];
    let i = 0;
    while (i < lines.length) {
      const line = lines[i];
      const fence = line.match(FENCE);
      if (fence) {
        let end = i + 1;
        while (end < lines.length && !new RegExp('^\\s*' + fence[1]).test(lines[end])) end++;
        end = Math.min(end + 1, lines.length);
        blocks.push({ start: i, end, kind: 'code' });
        i = end;
        continue;
      }
      const math = line.trim();
      const env = math.match(MATH_ENV);
      const closer = env ? `\\end{${env[1]}}` : math.startsWith('\\[') && !math.slice(2).trim() ? '\\]' : null;
      if (closer && !(env && math.includes(closer))) {
        let end = i + 1;
        while (end < lines.length && !lines[end].includes(closer)) end++;
        end = Math.min(end + 1, lines.length);
        blocks.push({ start: i, end, kind: 'math' });
        i = end;
        continue;
      }
      if (math.startsWith('$$') && !MATH_THEN.test(math) && !(math.length > 4 && math.endsWith('$$'))) {
        let end = i + 1;
        while (end < lines.length && !MATH_END.test(lines[end])) end++;
        end = Math.min(end + 1, lines.length);
        blocks.push({ start: i, end, kind: 'math' });
        i = end;
        continue;
      }
      if (line.includes('|') && i + 1 < lines.length && TABLE_RULE.test(lines[i + 1]) && lines[i + 1].includes('-')) {
        let end = i + 2;
        while (end < lines.length && lines[end].includes('|') && lines[end].trim()) end++;
        blocks.push({ start: i, end, kind: 'table' });
        i = end;
        continue;
      }
      if (QUOTE.test(line)) {
        let end = i + 1;
        while (end < lines.length && QUOTE.test(lines[end])) end++;
        blocks.push({ start: i, end, kind: 'quote' });
        i = end;
        continue;
      }
      blocks.push({ start: i, end: i + 1, kind: 'line' });
      i++;
    }
    if (!blocks.length) blocks.push({ start: 0, end: 1, kind: 'line' });
    return blocks;
  }

  function headings(lines) {
    const out = [];
    for (const [index, block] of splitBlocks(lines).entries()) {
      if (block.kind !== 'line') continue;
      const found = lines[block.start].match(/^\s*(#{1,6})\s+(.+?)\s*$/);
      if (!found) continue;
      const text = found[2].replace(/\s+#+\s*$/, '').trim();
      if (text) out.push({ line: block.start, level: found[1].length, text, block: index });
    }
    return out;
  }

  function headingTree(rows) {
    const tree = [];
    const stack = [];
    for (const row of rows) {
      if (!Number.isInteger(row.level) || row.level < 1 || row.level > 6) continue;
      const node = { ...row, children: [] };
      while (stack.length && stack[stack.length - 1].level >= row.level) stack.pop();
      if (stack.length) stack[stack.length - 1].children.push(node);
      else tree.push(node);
      stack.push(node);
    }
    return tree;
  }

  function boxesBefore(lines, index) {
    let count = 0;
    for (let i = 0; i < index && i < lines.length; i++) if (isBox(lines[i])) count++;
    return count;
  }

  function boxLine(lines, number) {
    let count = 0;
    for (let i = 0; i < lines.length; i++) {
      if (isBox(lines[i]) && ++count === number) return i;
    }
    return -1;
  }

  const ITEM = new RegExp(String.raw`^(?<lead>[^\S\n]*)(?<marker>${MARKER})(?<gap>[^\S\n]+)(?<box>\[[ xX]\](?:[^\S\n]+|$))?`);
  const HEAD = /^\s{0,3}#{1,6}[^\S\n]+/;

  function item(line) {
    const m = line.match(ITEM);
    return m && { lead: m.groups.lead, marker: m.groups.marker, gap: m.groups.gap, box: m.groups.box || '', head: m[0], text: line.slice(m[0].length) };
  }

  function listed(lines, at) {
    let from = at;
    while (from > 0 && item(lines[from - 1])) from--;
    let after = false;
    for (let i = from; i <= at; i++) after = Boolean(listable(lines[i], after));
    return after;
  }

  const itemIn = (line, above) => (!above || listable(line, above.length > 0 && listed(above, above.length - 1)) ? item(line) : null);
  const itemHead = (line, above) => itemIn(line, above)?.head || line.match(HEAD)?.[0] || '';
  const leadWidth = (lead) => lead.replace(/\t/g, '    ').length;

  function listLevels(lines) {
    const open = [];
    let after = false;
    return lines.map((line) => {
      after = Boolean(listable(line, after));
      if (!after) {
        open.length = 0;
        return -1;
      }
      const depth = leadWidth(item(line).lead);
      while (open.length && open[open.length - 1] >= depth) open.pop();
      open.push(depth);
      return open.length - 1 + Math.floor(open[0] / 2);
    });
  }

  const listPrefix = (line, above) => itemIn(line, above)?.head || '';
  function listPrefixAt(line, lines, start) {
    let from = start - 1;
    while (from > 0 && item(lines[from - 1])) from--;
    return listPrefix(line, lines.slice(Math.max(from, 0), start));
  }

  function nextMarker(marker) {
    const number = marker.match(/^(\d+)(.)$/);
    if (number) return `${Number(number[1]) + 1}${number[2]}`;
    const letters = marker.match(/^([a-zA-Z]+)(.)$/);
    if (!letters) return marker;
    const [, run, end] = letters;
    if (/[zZ]/.test(run)) return (run[0] === 'z' ? 'a' : 'A').repeat(run.length + 1) + end;
    return String.fromCharCode(run.charCodeAt(0) + 1).repeat(run.length) + end;
  }

  function place(marker) {
    const run = marker.slice(0, -1);
    return /\d/.test(run) ? Number(run) : run.toLowerCase().charCodeAt(0) - 96 + (run.length - 1) * 26;
  }

  const gap = (marker, previous) => place(marker) - place(previous);
  const closeAfter = (marker, previous) => gap(marker, previous) === 0 || gap(marker, previous) === 1 || (gap(marker, previous) === 2 && /\d/.test(marker));
  const exact = (marker, previous) => gap(marker, previous) === 1;

  function renumber(lines, at, fresh = -1) {
    let after = false;
    const live = lines.map((line) => (after = Boolean(listable(line, after))));
    if (!live[at]) return;
    let from = at;
    while (from > 0 && live[from - 1]) from--;
    const open = [];
    for (let i = from; i < lines.length; i++) {
      if (!live[i]) break;
      const found = item(lines[i]);
      const depth = leadWidth(found.lead);
      while (open.length && open[open.length - 1].depth > depth) open.pop();
      const level = open[open.length - 1]?.depth === depth ? open.pop() : null;
      const kind = listKind(found.marker);
      const same = level && kind !== 'ul' && listKind(level.marker) === kind;
      const early = i < at && i !== fresh;
      const near = same && level.last <= at && at <= i;
      const next = near ? closeAfter(found.marker, level.marker) || closeAfter(found.marker, level.typed)
        : same && (exact(found.marker, level.typed) || (found.marker === level.typed && level.typed === level.before));
      const chosen = i === fresh && /\d/.test(found.marker) && place(found.marker) > 99;
      const follows = !early && same && !chosen && (i === fresh || (!level.stopped && next));
      const starts = !same && i === fresh && !chosen && open.length > 0 && kind !== 'ul';
      const marker = follows ? nextMarker(level.marker) : starts ? kind : found.marker;
      const stopped = same ? !follows && !early : /[a-z]/i.test(kind) && !/^[aA][.)]/.test(marker);
      open.push({ depth, marker, typed: found.marker, before: level?.typed, last: i, stopped });
      if (marker !== found.marker) lines[i] = found.lead + marker + lines[i].slice(found.lead.length + found.marker.length);
    }
  }

  function closeList(lines, at) {
    if (item(lines[at] ?? '')) return renumber(lines, at);
    const rest = lines.slice();
    rest.splice(at, 1);
    renumber(rest, at);
    rest.splice(at, 0, lines[at]);
    lines.splice(0, lines.length, ...rest);
  }

  function stepBack(line, above) {
    const found = itemIn(line, above);
    if (!found) return line.slice(itemHead(line, above).length);
    return found.lead ? outdent(line, above) : found.text;
  }

  function continueLine(line, above) {
    const found = itemIn(line, above);
    if (found) {
      const prefix = `${found.lead}${nextMarker(found.marker)}${found.gap}${found.box ? '[ ] ' : ''}`;
      return found.text.trim() ? { prefix, exit: false } : { prefix, exit: true, back: stepBack(line, above) };
    }
    if (HEAD.test(line) && line === itemHead(line)) return { prefix: '', exit: true, back: '' };
    const m = line.match(/^(\s*)>\s?(.*)$/);
    if (m) return m[2].trim() ? { prefix: `${m[1]}> `, exit: false } : { prefix: '', exit: true, back: '' };
    return { prefix: '', exit: false };
  }

  function toggleBox(line) {
    if (!isBox(line)) return line;
    const at = line.indexOf('[') + 1;
    return line.slice(0, at) + (line[at] === ' ' ? 'x' : ' ') + line.slice(at + 1);
  }

  function indent(line, above = []) {
    const found = itemIn(line, above);
    if (!found) return '  ' + line;
    const depth = leadWidth(found.lead);
    let sibling = found;
    for (let k = above.length - 1; k >= 0; k--) {
      const earlier = item(above[k]);
      if (!earlier) break;
      const width = leadWidth(earlier.lead);
      if (width > depth) continue;
      if (width === depth) sibling = earlier;
      break;
    }
    return ' '.repeat(depth + sibling.marker.length + sibling.gap.length) + line.slice(found.lead.length);
  }

  function outdent(line, above = []) {
    const found = itemIn(line, above);
    if (found) {
      for (let k = above.length - 1; k >= 0; k--) {
        const earlier = item(above[k]);
        if (!earlier) break;
        if (leadWidth(earlier.lead) < leadWidth(found.lead)) return earlier.lead + line.slice(found.lead.length);
      }
      const own = found.marker.length + found.gap.length;
      const lead = leadWidth(found.lead);
      if (lead && !found.lead.includes('\t') && lead % own === 0) return ' '.repeat(lead - own) + line.slice(found.lead.length);
    }
    return line.replace(/^(?: {1,2}|\t)/, '');
  }

  const PREFIX = new RegExp(String.raw`^(\s*)(#{1,6}\s+|>\s?|(?:${MARKER})\s+(?:\[[ xX]\]\s+)?)?`);
  const TAG = /^<\/?u>/;
  const EMBED = /^!\[\[(?!\s*(?:data:image\/svg\+xml[;,]|[a-z][a-z0-9+.-]*:|\/\/))[^\]|[]+(?:\|[^\]]*)?\]\]/i;
  const DRAWN = /^\s*<?(?:data:image\/svg\+xml[;,]|(?![a-z][a-z0-9+.-]*:|\/\/)\S)/i;

  function hiddenHead(raw, isItem) {
    const found = raw.match(PREFIX);
    return isItem || !/^[a-z]/i.test(found[2] || '') ? found[0].length : found[1].length;
  }

  function visibleIndices(raw, isItem) {
    const out = [];
    const prefix = hiddenHead(raw, isItem);
    let first = -1;
    let i = prefix;
    while (i < raw.length) {
      const c = raw[i];
      const embed = c === '!' && raw.slice(i).match(EMBED);
      if (c === '[' || (c === '!' && raw[i + 1] === '[')) {
        const open = c === '!' ? i + 1 : i;
        const close = raw.indexOf('](', open);
        const end = embed ? i + embed[0].length - 1 : close >= 0 ? raw.indexOf(')', close) : -1;
        if (embed || (close >= 0 && end >= 0)) {
          if (embed || (c === '!' && DRAWN.test(raw.slice(close + 2, end)))) {
            if (!out.length) first = i;
            out.push(end);
          } else for (let k = open + 1; k < close; k++) out.push(k);
          i = end + 1;
          continue;
        }
      }
      const tag = c === '<' && raw.slice(i).match(TAG);
      if (tag) {
        i += tag[0].length;
        continue;
      }
      // shortcut: every `*` and word-edge `_` counts as a mark, so a caret lands one character off past an unpaired one; read the renderer's pairs if that shows.
      if (c === '*' || c === '~' || c === '`' || (c === '_' && !(/\w/.test(raw[i - 1] || '') && /\w/.test(raw[i + 1] || '')))) {
        i++;
        continue;
      }
      out.push(i);
      i++;
    }
    return { prefix, out, first: first < 0 ? out[0] : first };
  }

  function rawOffset(raw, visiblePrefix, isItem = true) {
    const { prefix, out, first } = visibleIndices(raw, isItem);
    const k = visiblePrefix.length;
    if (k === 0) return out.length ? first : prefix;
    if (k >= out.length) return raw.length;
    return out[k - 1] + 1;
  }

  const STYLES = { bold: 2, italic: 1 };
  const CALLOUT = /^\s*>\s*\[![A-Za-z-]+\][+-]?/;
  const wordy = (c) => /[\p{L}\p{N}]/u.test(c || '');

  function narrowed(line, lo, hi, edges, inside) {
    const inner = line.slice(lo, hi);
    const bare = inner.replace(edges, '');
    if (!bare || inside.test(bare)) return [lo, hi];
    const from = lo + inner.indexOf(bare);
    return [from, from + bare.length];
  }

  function styleRegion(line, lo, hi, style) {
    if (style === 'underline') {
      [lo, hi] = narrowed(line, lo, hi, /^[*_~]*(?:<u>)?[*_~]*|[*_~]*(?:<\/u>)?[*_~]*$/g, /<\/?u>/);
      const before = line.slice(0, lo).match(/<u>([*_~]*)$/);
      const after = line.slice(hi).match(/^([*_~]*)<\/u>/);
      return before && after ? { lo, hi, before: 3 + before[1].length, after: 4 + after[1].length, keep: [before[1], after[1]], on: true } : { lo, hi, on: false };
    }
    [lo, hi] = narrowed(line, lo, hi, /^[*_]+|[*_]+$/g, /[*_]/);
    const left = line.slice(0, lo).match(/(\*{1,3}|_{1,3})$/)?.[0] || '';
    const right = line.slice(hi).match(/^(\*{1,3}|_{1,3})/)?.[0] || '';
    const opens = left[0] === '_' ? !wordy(line[lo - left.length - 1]) : left.length > 1 || !wordy(line[lo - left.length - 1]);
    const shuts = right[0] !== '_' || !wordy(line[hi + right.length]);
    const paired = left && left[0] === right[0];
    const run = paired && opens && shuts ? Math.min(left.length, right.length) : 0;
    return { lo, hi, before: STYLES[style], after: STYLES[style], on: style === 'bold' ? run >= 2 : run % 2 === 1, stuck: paired && !run && left[0] === '*' };
  }

  function restyle(text, from, to, style) {
    const rows = text.split('\n');
    const regions = [];
    const skipped = new Set();
    if (text.slice(from, to).includes('\n')) {
      for (const block of splitBlocks(rows)) if (!['line', 'quote'].includes(block.kind)) for (let row = block.start; row < block.end; row++) skipped.add(row);
    }
    const levels = listLevels(rows);
    let base = 0;
    rows.forEach((line, row) => {
      let lo = Math.max(from - base, hiddenHead(line, levels[row] >= 0), line.match(CALLOUT)?.[0].length || 0);
      let hi = Math.min(to - base, line.length);
      while (lo < hi && /\s/.test(line[lo])) lo++;
      while (lo < hi && /\s/.test(line[hi - 1])) hi--;
      if (lo < hi && !skipped.has(row) && /[\p{L}\p{N}]/u.test(line.slice(lo, hi))) {
        const region = styleRegion(line, lo, hi, style);
        if (!region.stuck) regions.push({ row, ...region });
      }
      base += line.length + 1;
    });
    if (!regions.length) return { text, from, to };
    const off = regions.every((region) => region.on);
    const open = style === 'underline' ? '<u>' : '*'.repeat(STYLES[style]);
    const close = style === 'underline' ? '</u>' : open;
    const spots = regions.map(({ row, lo, hi, before, after, keep = ['', ''], on }) => {
      const line = rows[row];
      if (on !== off) return { row, lo, hi };
      if (off) {
        rows[row] = line.slice(0, lo - before) + keep[0] + line.slice(lo, hi) + keep[1] + line.slice(hi + after);
        return { row, lo: lo - before + keep[0].length, hi: hi - before + keep[0].length };
      }
      rows[row] = line.slice(0, lo) + open + line.slice(lo, hi) + close + line.slice(hi);
      return { row, lo: lo + open.length, hi: hi + open.length };
    });
    const offset = (row, column) => rows.slice(0, row).reduce((sum, line) => sum + line.length + 1, column);
    const [first, last] = [spots[0], spots[spots.length - 1]];
    const marked = !off && first.row !== last.row;
    return { text: rows.join('\n'), from: offset(first.row, first.lo) - (marked ? open.length : 0), to: offset(last.row, last.hi) + (marked ? close.length : 0) };
  }

  function stylePair(text, at, style) {
    const before = text.slice(0, at);
    const after = text.slice(at);
    const open = style === 'underline' ? '<u>' : '*'.repeat(STYLES[style]);
    const close = style === 'underline' ? '</u>' : open;
    const line = before.slice(before.lastIndexOf('\n') + 1);
    const insert = { text: before + open + close + after, at: at + open.length };
    if (style === 'underline') {
      const found = styleRegion(text, at, at, style);
      if (found.on) return { text: before.slice(0, at - found.before) + found.keep.join('') + after.slice(found.after), at: at - found.before + found.keep[0].length };
      return after.startsWith(close) && line.includes(open) ? { text, at: at + close.length } : insert;
    }
    const left = before.match(/\*+$/)?.[0].length || 0;
    const right = after.match(/^\*+/)?.[0].length || 0;
    const closes = (run) => (style === 'bold' ? run >= 2 : run % 2 === 1);
    if (left >= right && closes(right)) return { text: before.slice(0, at - open.length) + after.slice(close.length), at: at - open.length };
    return closes(right) && line.includes(open) ? { text, at: at + close.length } : insert;
  }

  function tableCells(text) {
    const found = [];
    let base = 0;
    text.split('\n').forEach((row, line) => {
      if (line !== 1 && row.trim()) {
        const bars = [...row.matchAll(/(?<!\\)\|/g)].map((m) => m.index);
        if (!/^\s*\|/.test(row)) bars.unshift(-1);
        if (!/(?<!\\)\|\s*$/.test(row)) bars.push(row.length);
        for (let col = 0; col + 1 < bars.length; col++) {
          const lo = bars[col] + 1;
          const hi = bars[col + 1];
          const inner = row.slice(lo, hi);
          const from = lo + (inner.trim() ? inner.length - inner.trimStart().length : Math.min(1, inner.length));
          const to = from + inner.trim().length;
          found.push({ line, col, lo: base + lo, hi: base + hi, from: base + from, to: base + to });
        }
      }
      base += row.length + 1;
    });
    return found;
  }

  function cellEnd(text, row, col) {
    return tableCells(text).find((cell) => cell.line === (row ? row + 1 : 0) && cell.col === col)?.to ?? 'end';
  }

  function nextCell(text, at, back) {
    const all = tableCells(text);
    const here = all.findIndex((cell) => at <= cell.hi);
    if (back) return all[Math.max(0, here - 1)] || null;
    return here >= 0 ? all[here + 1] || null : null;
  }

  function addRow(text) {
    const columns = tableCells(text).filter((cell) => cell.line === 0).length;
    const body = text.replace(/\s+$/, '');
    return { text: `${body}\n|${'  |'.repeat(columns)}`, at: body.length + 3 };
  }

  const MOST_PASSAGE = 8000;

  function lineAtOffset(lines, pos) {
    let start = 0;
    for (let k = 0; k < lines.length; k++) {
      if (pos <= start + lines[k].length) return k;
      start += lines[k].length + 1;
    }
    return lines.length - 1;
  }

  function snapshotOf(lines, from, to) {
    const text = lines.join('\n');
    let a = Math.max(0, Math.min(from, text.length));
    let b = Math.max(0, Math.min(to, text.length));
    if (a > b) [a, b] = [b, a];
    const picked = text.slice(a, b);
    if (!picked.trim()) return null;
    return {
      from: lineAtOffset(lines, a) + 1,
      to: lineAtOffset(lines, Math.max(a, b - 1)) + 1,
      text: [...picked].slice(0, MOST_PASSAGE).join(''),
    };
  }

  const PICTURE = /!\[\[([^\]|]+)(?:\|([^\]]*))?\]\]|!\[([^\]]*)\]\((<[^>]+>|[^)\s]+)(\s+"[^"]*")?\)/g;

  const outside = (target) => {
    const src = String(target || '').trim().replace(/^<(.*)>$/, '$1');
    return !/^data:image\/svg\+xml[;,]/i.test(src) && (/^[a-z][a-z0-9+.-]*:/i.test(src) || src.startsWith('//'));
  };

  function changePicture(text, index, change) {
    let n = -1;
    const code = [...String(text).matchAll(/`[^`]+`/g)].map((m) => [m.index, m.index + m[0].length]);
    return String(text).replace(PICTURE, (whole, name, option, alt, target, title, at) => {
      if (code.some(([from, to]) => at >= from && at < to) || outside(name ?? target)) return whole;
      n += 1;
      return n === index ? change(name, option, alt, target, title, at) : whole;
    });
  }

  function setPictureWidth(text, index, width) {
    const size = width ? `|${Math.round(width)}` : '';
    return changePicture(text, index, (name, option, alt, target, title) => {
      if (name !== undefined) {
        const keep = option && !/^\s*\d{1,4}(?:\s*x\s*\d{1,4})?\s*$/.test(option) ? `|${option}` : '';
        return `![[${name}${size || keep}]]`;
      }
      return `![${alt.replace(/\|\s*\d{1,4}(?:\s*x\s*\d{1,4})?\s*$/, '')}${size}](${target}${title || ''})`;
    });
  }

  function removePicture(text, index) {
    let at = -1;
    const rest = changePicture(text, index, (name, option, alt, target, title, found) => {
      at = found;
      return '';
    });
    return { text: rest, at };
  }

  const SLASH = [
    { text: 'Heading', mark: '# ' }, { text: 'Subheading', mark: '## ' }, { text: 'Small heading', mark: '### ' },
    { text: 'Bullet', mark: '- ' }, { text: 'Numbered list', mark: '1. ' }, { text: 'Checkbox', mark: '- [ ] ' },
    { text: 'Quote', mark: '> ' }, { text: 'Warning box', mark: '> [!warning] ' }, { text: 'Note box', mark: '> [!note] ' },
    { text: 'Tip box', mark: '> [!tip] ' }, { text: 'Code block', mark: '```' }, { text: 'Divider', mark: '---' },
  ];

  function slash(line, above, at, end, mark) {
    const before = line.slice(0, at);
    const after = line.slice(end);
    if (/^:[a-z]+:$/.test(mark)) {
      const icon = mark + (/^\s/.test(after) ? '' : ' ');
      return { lines: [before + icon + after], line: 0, caret: before.length + icon.length };
    }
    const joined = before + after;
    const head = itemHead(joined, above) || joined.match(/^\s*>[^\S\n]?(?:\[![A-Za-z-]+\][+-]?[^\S\n]*)?/)?.[0] || '';
    const text = joined.slice(head.length);
    const caret = Math.max(0, before.length - head.length);
    if (mark === '```') return { lines: ['```', text, '```'], line: 1, caret };
    if (mark === '---') return above.length && above[above.length - 1].trim() ? { lines: ['', '---', text], line: 2, caret } : { lines: ['---', text], line: 1, caret };
    const lead = /^[-1]/.test(mark) ? head.match(/^\s*/)[0] : '';
    return { lines: [lead + mark + text], line: 0, caret: lead.length + mark.length + caret };
  }

  const api = { SLASH, slash, restyle, stylePair, cellEnd, nextCell, addRow, listPrefix, listPrefixAt, itemHead, listed, listLevels, renumber, closeList, stepBack, headingTree, headings, setPictureWidth, removePicture, splitBlocks, boxesBefore, boxLine, continueLine, toggleBox, indent, outdent, rawOffset, isBox, snapshotOf, lineAtOffset, MOST_PASSAGE };
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
  else root.leoEditing = api;
})(typeof window !== 'undefined' ? window : globalThis);
