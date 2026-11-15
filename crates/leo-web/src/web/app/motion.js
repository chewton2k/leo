const FLIP_MS = 150;
const FLIP_TRAVEL = 40;
const FLIP_ROWS = 200;
const QUICK_FLIP_MS = 120;
const SETTLE_MS = 180;
const SETTLE_SCALE = 1.02;

const drawn = { key: '', top: false };

function scrollToTop() {
  if (!drawn.top) return;
  drawn.top = false;
  window.scrollTo(0, 0);
}

function flipOffset(before, after, travel = FLIP_TRAVEL) {
  const dx = before.left - after.left;
  const dy = before.top - after.top;
  if (Math.abs(dx) < 1 && Math.abs(dy) < 1) return null;
  const cap = (value) => Math.max(-travel, Math.min(travel, value));
  return { dx: cap(dx), dy: cap(dy) };
}

function flipMoves(before, after, travel = FLIP_TRAVEL) {
  const moves = [];
  for (const [key, to] of after) {
    const from = before.get(key);
    const offset = from && flipOffset(from, to, travel);
    if (offset) moves.push({ key, ...offset });
  }
  return moves;
}

function flipSkip({ reduced, count, changed = true, limit = FLIP_ROWS }) {
  return Boolean(reduced) || !changed || count < 2 || count > limit;
}

function flipRects(els, keyOf) {
  const rects = new Map();
  for (const el of els) {
    const key = keyOf(el);
    if (key === undefined || key === null || rects.has(key)) continue;
    const { left, top } = el.getBoundingClientRect();
    rects.set(key, { left, top });
  }
  return rects;
}

function flipPlay(els, keyOf, before, ms) {
  const after = flipRects(els, keyOf);
  const moves = new Map(flipMoves(before, after).map((move) => [move.key, move]));
  if (!moves.size) return 0;
  let played = 0;
  for (const el of els) {
    const move = moves.get(keyOf(el));
    if (!move || typeof el.animate !== 'function') continue;
    el.animate([{ transform: `translate(${move.dx}px, ${move.dy}px)` }, { transform: 'none' }], { duration: ms, easing: EASE_MOVE });
    played++;
  }
  return played;
}

function dockPanel(el, box = null) {
  if (!el || stillMotion() || typeof el.animate !== 'function') return null;
  const from = el.getBoundingClientRect().top;
  const side = box ? box.getBoundingClientRect() : null;
  return () => {
    const dy = from - el.getBoundingClientRect().top;
    if (Math.abs(dy) >= 1) el.animate([{ transform: `translateY(${dy}px)` }, { transform: 'none' }], { duration: DRAWER_MS, easing: EASE_MOVE });
    const to = side ? box.getBoundingClientRect() : null;
    if (to && Math.abs(side.width - to.width) >= 1) {
      const dx = side.left - to.left;
      box.animate([{ transform: `translateX(${dx}px)`, width: `${side.width}px` }, { transform: 'none', width: `${to.width}px` }], { duration: DRAWER_MS, easing: EASE_MOVE });
    }
  };
}

function flipList(root, selector, keyOf, change, { ms = FLIP_MS, changed = true } = {}) {
  const els = root ? [...root.querySelectorAll(selector)] : [];
  if (flipSkip({ reduced: stillMotion(), count: els.length, changed })) return change();
  const before = flipRects(els, keyOf);
  const result = change();
  flipPlay([...root.querySelectorAll(selector)], keyOf, before, ms);
  return result;
}

function transitionSkip({ reduced, sheet, same, first }) {
  if (reduced) return 'reduced';
  if (sheet) return 'sheet';
  if (first) return 'first';
  if (same) return 'same';
  return '';
}

function swap(key, draw) {
  const skip = transitionSkip({
    reduced: stillMotion(),
    sheet: Boolean(document.querySelector('.scrim')),
    same: drawn.key === key,
    first: !drawn.key,
  });
  drawn.key = key;
  draw();
  return !skip;
}

const ARC_STEPS = 12;

function arcPoints() {
  const ease = (t) => t * t * (3 - 2 * t);
  return Array.from({ length: ARC_STEPS + 1 }, (_, i) => {
    const x = ease(i / ARC_STEPS);
    return { x, y: x ** 1.6 };
  });
}

function settleOffset(from, to) {
  if (!from) return { dx: 0, dy: 0 };
  return { dx: from.left - to.left, dy: from.top - to.top };
}

function settle(el, { from = null, elapsed = 0, ms = SETTLE_MS } = {}) {
  if (!el || stillMotion() || typeof el.animate !== 'function' || elapsed >= ms) return null;
  const to = el.getBoundingClientRect();
  const { dx, dy } = settleOffset(from, to);
  settleLayer?.cancel();
  const flyer = from && clippedBy(el) ? settleCarrier(el, to) : null;
  const tilt = from?.tilt ? (x) => ` rotate(${Number((from.tilt * (1 - x)).toFixed(4))}deg)` : () => '';
  const frames = arcPoints().map(({ x, y }) => ({ transform: `translate(${dx * (1 - x)}px, ${dy * (1 - y)}px) scale(${1 + Number(((SETTLE_SCALE - 1) * (1 - x)).toFixed(4))})${tilt(x)}` }));
  frames[0].boxShadow = '0 10px 28px rgba(0, 0, 0, .18), 0 2px 6px rgba(0, 0, 0, .1)';
  frames[ARC_STEPS] = { transform: 'none', boxShadow: '0 0 0 rgba(0, 0, 0, 0)' };
  const run = (flyer || el).animate(frames, { duration: ms, easing: 'linear' });
  run.currentTime = elapsed;
  if (flyer) {
    settleLayer = run;
    run.onfinish = run.oncancel = () => {
      flyer.remove();
      el.style.visibility = '';
      if (settleLayer === run) settleLayer = null;
    };
  }
  return run;
}

function clippedBy(el) {
  for (let up = el.parentElement; up && up !== document.body; up = up.parentElement) if (getComputedStyle(up).overflow !== 'visible') return true;
  return false;
}

function settleCarrier(el, to) {
  const flyer = ghostOf(el);
  const look = getComputedStyle(el);
  Object.assign(flyer.style, { position: 'fixed', zIndex: 1000, left: `${to.left}px`, top: `${to.top}px`, width: `${to.width}px`, height: `${to.height}px`, margin: 0, pointerEvents: 'none', background: look.backgroundColor, borderRadius: look.borderRadius });
  document.body.appendChild(flyer);
  el.style.visibility = 'hidden';
  return flyer;
}

function flyInto(ghost, target) {
  if (!ghost) return;
  if (!target || stillMotion() || typeof ghost.animate !== 'function') return ghost.remove();
  const from = ghost.getBoundingClientRect(), to = target.getBoundingClientRect();
  const dx = to.left + to.width / 2 - (from.left + from.width / 2);
  const dy = to.top + to.height / 2 - (from.top + from.height / 2);
  const shrink = Math.min(1, to.height / from.height, to.width / from.width);
  const left = from.left + (from.width - ghost.offsetWidth) / 2, top = from.top + (from.height - ghost.offsetHeight) / 2;
  const frames = arcPoints().map(({ x, y }) => ({ transform: `translate(${left + dx * x}px, ${top + dy * y}px) scale(${1 + (shrink - 1) * x})`, opacity: x < .7 ? .96 : .96 * (1 - x) / .3 }));
  ghost.animate(frames, { duration: DRAWER_MS, easing: 'linear', fill: 'forwards' }).onfinish = () => {
    ghost.remove();
    pulse(target);
  };
}

const handoff = { pending: null };
let settleLayer = null;

function settleLater(kind, id, from) {
  handoff.pending = { kind, id, from, at: performance.now() };
}

function settleNow(kind, find) {
  const pending = handoff.pending;
  if (!pending || pending.kind !== kind) return;
  const el = find(pending.id);
  const elapsed = performance.now() - pending.at;
  if (elapsed >= SETTLE_MS) {
    handoff.pending = null;
    return;
  }
  if (!el) return;
  if (el.dataset.settled !== String(pending.at)) {
    el.dataset.settled = String(pending.at);
    settle(el, { from: pending.from, elapsed });
  }
}

const PILL_MS = DRAWER_MS;

function pillKey(pill) {
  const row = pill && pill.parentElement;
  return row ? row.dataset.id || row.dataset.refreshKey || '' : '';
}

function pillGlide(from, pill, ms = PILL_MS) {
  if (!from || !pill || stillMotion() || typeof pill.animate !== 'function') return null;
  const to = pill.getBoundingClientRect();
  if (!to.width || !to.height) return null;
  const dy = from.top - to.top;
  const dx = from.left - to.left;
  const sx = from.width / to.width;
  const sy = from.height / to.height;
  if (Math.abs(dx) < 1 && Math.abs(dy) < 1 && Math.abs(sx - 1) < .01 && Math.abs(sy - 1) < .01) return null;
  return pill.animate([
    { transform: `translate(${dx}px, ${dy}px) scale(${sx}, ${sy})` },
    { transform: 'none' },
  ], { duration: ms, easing: EASE_MOVE });
}

function lift(el) {
  if (!el || stillMotion() || typeof el.animate !== 'function') return null;
  return el.animate([{ scale: '1', boxShadow: 'none', '--tilt': '0deg' }], { duration: MOTION_MS * EXIT_SHARE, easing: EASE_OUT });
}

function liftedRect(el) {
  if (!el) return null;
  const r = el.getBoundingClientRect();
  return { left: r.left + (r.width - el.offsetWidth) / 2, top: r.top + (r.height - el.offsetHeight) / 2, tilt: parseFloat(getComputedStyle(el).getPropertyValue('--tilt')) || 0 };
}

window.leoMotion = { unfold, fold, cascade, pulse, rubber, iconSwap, overlapOut, stillMotion, dock: dockPanel };
