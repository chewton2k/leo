(function (root) {
  'use strict';

  const MARGIN_WIDTH = 240;
  const MOST_MARGIN = 360;
  const LEAST_SHARED = 180;
  const SHARED_COLUMN = 520;
  const MARGIN_GAP = 24;
  const TICK_GUTTER = 48;
  const END_GUTTER = 16;
  const LEAST_COLUMN = 600;
  const TAIL = 24;
  const LEAST_TAIL = 6;
  const READ_BLOCKS = 6;
  const READ_BLOCK_CHARS = 4000;
  const READ_CHARS = 8000;
  const LEAST_BLOCK_CHARS = 20;
  const MOST_PASSES = 8;
  const NEAR = 120;
  const AROUND = 48;
  const WORD_MS = 62;
  const STAGGER_MS = 650;
  const READ_PAUSE = { fast: 2000, regular: 4000, slow: 8000 };
  const UNFINISHED_PAUSE = 1.5;

  function occurrences(source, text) {
    const hits = [];
    let at = source.indexOf(text);
    while (at >= 0) {
      hits.push(at);
      at = source.indexOf(text, at + 1);
    }
    return hits;
  }

  function sharedEnd(a, b) {
    let n = 0;
    while (n < a.length && n < b.length && a[a.length - 1 - n] === b[b.length - 1 - n]) n++;
    return n;
  }

  function sharedStart(a, b) {
    let n = 0;
    while (n < a.length && n < b.length && a[n] === b[n]) n++;
    return n;
  }

  function byContext(source, quote, prefix, suffix) {
    const before = prefix.slice(-TAIL);
    const after = suffix.slice(0, TAIL);
    const room = quote.length * 2 + NEAR;
    const enough = (tail, whole) => !whole || tail.length >= LEAST_TAIL;
    if (!enough(before, prefix) || !enough(after, suffix)) return null;
    if (!before && !after) return null;
    const starts = before ? occurrences(source, before).map((at) => at + before.length) : [0];
    for (const from of starts) {
      const to = after ? source.indexOf(after, from + 1) : source.length;
      if (to > from && to - from <= room && source.slice(from, to).trim()) return { from, to, how: 'context' };
    }
    return null;
  }

  function find(source, { quote = '', prefix = '', suffix = '' }) {
    if (!quote) return null;
    const hits = occurrences(source, quote);
    if (hits.length === 1) return { from: hits[0], to: hits[0] + quote.length, how: 'exact' };
    if (hits.length) {
      const read = (at) => {
        const before = sharedEnd(source.slice(Math.max(0, at - prefix.length), at), prefix);
        const after = sharedStart(source.slice(at + quote.length, at + quote.length + suffix.length), suffix);
        const sure = (prefix ? before >= Math.min(LEAST_TAIL, prefix.length) : at === 0) || (suffix ? after >= Math.min(LEAST_TAIL, suffix.length) : at + quote.length === source.length);
        return { at, score: before + after, sure };
      };
      const reads = hits.map(read);
      const best = reads.reduce((a, b) => (b.score > a.score ? b : a));
      const alone = reads.every((one) => one === best || one.score < best.score);
      return { from: best.at, to: best.at + quote.length, how: best.sure && alone ? 'exact' : 'guess' };
    }
    return byContext(source, quote, prefix, suffix);
  }

  function around(source, quote, line = 1) {
    const start = source.split('\n').slice(0, Math.max(0, line - 1)).join('\n').length;
    const first = source.indexOf(quote, line > 1 ? start : 0);
    const at = first >= 0 ? first : source.indexOf(quote);
    if (at < 0) return null;
    const low = (code) => code >= 0xdc00 && code <= 0xdfff;
    const high = (code) => code >= 0xd800 && code <= 0xdbff;
    let from = Math.max(0, at - AROUND);
    let to = Math.min(source.length, at + quote.length + AROUND);
    if (from > 0 && low(source.charCodeAt(from))) from++;
    if (to < source.length && high(source.charCodeAt(to - 1))) to--;
    return { prefix: source.slice(from, at), suffix: source.slice(at + quote.length, to) };
  }

  function lineAfter(source, at) {
    return source.slice(0, at).split('\n').length;
  }

  function stack(items, { gap = 16, floor = 0, pinned = null } = {}) {
    const sorted = [...items].sort((a, b) => a.want - b.want);
    const tops = [];
    let next = floor;
    for (const item of sorted) {
      const top = Math.max(item.want, next);
      tops.push(top);
      next = top + item.height + gap;
    }
    const held = sorted.findIndex((item) => item.id === pinned);
    if (held >= 0) {
      tops[held] = Math.max(sorted[held].want, floor);
      for (let i = held + 1; i < sorted.length; i++) tops[i] = Math.max(sorted[i].want, tops[i - 1] + sorted[i - 1].height + gap);
      for (let i = held - 1; i >= 0; i--) tops[i] = Math.min(tops[i], tops[i + 1] - sorted[i].height - gap);
      if (sorted.length && tops[0] < floor) {
        tops[0] = floor;
        for (let i = 1; i < sorted.length; i++) tops[i] = Math.max(tops[i], tops[i - 1] + sorted[i - 1].height + gap);
      }
    }
    return Object.fromEntries(sorted.map((item, i) => [item.id, tops[i]]));
  }

  function sides(items, gap = 16, before = {}) {
    const ends = { left: -Infinity, right: -Infinity };
    const out = {};
    for (const item of [...items].sort((a, b) => a.want - b.want)) {
      const kept = before[item.id];
      const free = (side) => ends[side] <= item.want;
      const sooner = ends.right <= ends.left ? 'right' : 'left';
      const side = kept && free(kept) ? kept : free('right') ? 'right' : free('left') ? 'left' : sooner;
      out[item.id] = side;
      ends[side] = Math.max(ends[side], item.want) + item.height + gap;
    }
    return out;
  }

  function clear(item, placed, both = false, gap = 16) {
    const hits = (side, top) => placed.filter((p) => p.side === side && p.top < top + item.height + gap && top < p.top + p.height + gap);
    if (!hits('right', item.want).length) return { side: 'right', top: item.want };
    if (both && !hits('left', item.want).length) return { side: 'left', top: item.want };
    let top = item.want;
    for (let blocked = hits('right', top); blocked.length; blocked = hits('right', top)) top = Math.max(...blocked.map((p) => p.top + p.height + gap));
    return { side: 'right', top };
  }

  const ROOM_RIGHT = TICK_GUTTER + LEAST_COLUMN + MARGIN_GAP + MARGIN_WIDTH + END_GUTTER;
  const ROOM_BOTH = TICK_GUTTER + SHARED_COLUMN + 2 * (MARGIN_GAP + LEAST_SHARED) + END_GUTTER;

  function room(width, column, sides) {
    return Math.floor((width - column - TICK_GUTTER - END_GUTTER) / sides) - MARGIN_GAP;
  }

  function arrangement({ width, setting = 'both', noteWidth = 'wide', panel = false }) {
    if (setting === 'marks' || noteWidth === 'full' || panel) return 'marks';
    return width < (setting === 'both' ? ROOM_BOTH : ROOM_RIGHT) ? 'marks' : setting;
  }

  function mostColumn({ width, setting = 'both', noteWidth = 'wide', panel = false }) {
    if (arrangement({ width, setting, noteWidth, panel }) === 'marks') return Infinity;
    const both = setting === 'both';
    return width - TICK_GUTTER - END_GUTTER - (both ? 2 : 1) * ((both ? LEAST_SHARED : MARGIN_WIDTH) + MARGIN_GAP);
  }

  function marginWidth({ width, column = Infinity, layout }) {
    const both = layout === 'both';
    return Math.min(MOST_MARGIN, Math.max(both ? LEAST_SHARED : MARGIN_WIDTH, room(width, column, both ? 2 : 1)));
  }

  function wordDelay(index, count) {
    return index * Math.min(WORD_MS, count > 1 ? STAGGER_MS / (count - 1) : 0);
  }

  function blocksOf(source) {
    return source.split(/\n\s*\n/).map((block) => block.trim()).filter(Boolean);
  }

  function pauseFor(lastBlock, pace) {
    const base = READ_PAUSE[pace] || READ_PAUSE.regular;
    const block = (lastBlock || '').trim();
    const line = block.split('\n').pop();
    const finished = !block || /^(```|~~~)/.test(block) || /^\s*(#{1,6}\s|[-*+]\s|\d+[.)]\s)/.test(line) || /[.!?。！？]["'”’)\]*_]*$/.test(block);
    return finished ? base : base * UNFINISHED_PAUSE;
  }

  function changedBlocks(before, source) {
    const known = new Map();
    for (const block of before) known.set(block, (known.get(block) || 0) + 1);
    const fresh = [];
    for (const block of blocksOf(source)) {
      const left = known.get(block) || 0;
      if (left) known.set(block, left - 1);
      else if ([...block].length >= LEAST_BLOCK_CHARS) fresh.push(block);
    }
    const out = [];
    let used = 0;
    for (const block of fresh.slice(-READ_BLOCKS).reverse()) {
      const clipped = [...block].slice(-READ_BLOCK_CHARS).join('').trim();
      const size = [...clipped].length;
      if (used + size > READ_CHARS) break;
      used += size;
      out.unshift(clipped);
    }
    return out;
  }

  function passes(source, taken = []) {
    const pieces = [];
    let at = 0;
    for (const seg of source.split(/(\n\s*\n)/)) {
      const block = seg.trim();
      const start = at + (block ? seg.indexOf(block) : 0);
      at += seg.length;
      if (!block || taken.some((quote) => quote && block.includes(quote))) continue;
      const cps = [...block];
      let used = 0;
      for (let i = 0; i < cps.length; i += READ_BLOCK_CHARS) {
        const raw = cps.slice(i, i + READ_BLOCK_CHARS).join('');
        const text = raw.trim();
        const from = start + used + raw.indexOf(text);
        used += raw.length;
        if ([...text].length >= LEAST_BLOCK_CHARS) pieces.push({ text, from, to: from + text.length, size: [...text].length });
      }
    }
    const all = [];
    let part = null;
    for (const piece of pieces) {
      if (!part || part.blocks.length >= READ_BLOCKS || part.size + piece.size > READ_CHARS) {
        part = { blocks: [], size: 0, start: piece.from, end: piece.to };
        all.push(part);
      }
      part.blocks.push(piece.text);
      part.size += piece.size;
      part.end = piece.to;
    }
    const spread = all.length > MOST_PASSES;
    const chosen = spread ? [...new Set(Array.from({ length: MOST_PASSES }, (_, i) => Math.round((i * (all.length - 1)) / (MOST_PASSES - 1))))] : all.map((_, i) => i);
    const parts = chosen.map((i) => ({ blocks: all[i].blocks, from: lineAfter(source, all[i].start), to: lineAfter(source, all[i].end) }));
    return { parts, spread };
  }

  function wave(left, right, y) {
    const count = Math.max(1, Math.round((right - left) / 8));
    const step = (right - left) / count;
    let path = `M${left.toFixed(1)} ${y.toFixed(1)}`;
    for (let i = 0; i < count; i++) path += ` q${(step / 2).toFixed(1)} ${i % 2 ? 3 : -3} ${step.toFixed(1)} 0`;
    return path;
  }

  const api = { MARGIN_WIDTH, MOST_MARGIN, LEAST_SHARED, SHARED_COLUMN, MARGIN_GAP, ROOM_RIGHT, ROOM_BOTH, LEAST_COLUMN, find, around, lineAfter, stack, sides, clear, arrangement, mostColumn, marginWidth, wordDelay, blocksOf, pauseFor, READ_PAUSE, changedBlocks, passes, wave };
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
  else root.leoMargin = api;
})(typeof window !== 'undefined' ? window : globalThis);
