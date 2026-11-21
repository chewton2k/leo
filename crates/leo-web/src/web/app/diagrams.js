const MERMAID_URL = '/vendor/mermaid-11.4.1.js';
const DIAGRAMS_KEPT = 60;
const DIAGRAM_WAITING = 'figure.diagram:not([data-drawn])';
const DRAWING_IMAGE = 'figure.drawing > img';
const diagrams = { loading: null, seq: 0, scheme: '', drawn: new Map(), timer: 0, inked: new WeakMap(), steps: new Map(), waiting: new Set(), images: new Set() };
const DRAWING_COLOURS = ['--text', '--muted', '--faint', '--line', '--accent', '--accent-soft', '--surface'];
const DRAWING_URL = 'data:image/svg+xml;charset=utf-8,';
const MOST_STEPS = 12;
const STEP_MS = 1100;
const STEP_CHAR_MS = 20;

function inked(svg, colour) {
  return svg.replace(/var\(\s*(--[\w-]+)\s*(?:,[^()]*)?\)/g, (all, name) => colour(DRAWING_COLOURS.includes(name) ? name : '--text'));
}

function drawingSteps(svg) {
  const doc = new DOMParser().parseFromString(svg, 'image/svg+xml');
  const root = doc.documentElement;
  if (!root || root.localName !== 'svg' || doc.getElementsByTagName('parsererror').length) return null;
  const stepOf = (g) => Number(g.getAttribute('data-step')) || 0;
  const groups = [...root.children].filter((g) => g.localName === 'g' && g.hasAttribute('data-step'));
  const numbers = [...new Set(groups.map(stepOf))].sort((a, b) => a - b);
  if (!root.hasAttribute('fill')) root.setAttribute('fill', 'var(--text)');
  if (!numbers.length || numbers.length > MOST_STEPS) return [{ svg: new XMLSerializer().serializeToString(root), caption: '' }];
  const kept = new Map(groups.map((g) => [g, g.getAttribute('opacity')]));
  return numbers.map((n) => {
    for (const g of groups) {
      if (stepOf(g) > n) g.setAttribute('opacity', '0');
      else if (kept.get(g) === null) g.removeAttribute('opacity');
      else g.setAttribute('opacity', kept.get(g));
    }
    return { svg: new XMLSerializer().serializeToString(root), caption: groups.find((g) => stepOf(g) === n).getAttribute('data-caption') || '' };
  });
}

function inkedUrl(svg) {
  const css = getComputedStyle(document.documentElement);
  return DRAWING_URL + encodeURIComponent(inked(svg, (name) => css.getPropertyValue(name).trim() || 'currentColor'));
}

function paintDrawing(fig, at) {
  const kept = diagrams.inked.get(fig);
  const last = kept.variants.length - 1;
  kept.at = Math.max(0, Math.min(last, at));
  const step = kept.variants[kept.at];
  fig.querySelector('img').src = inkedUrl(step.svg);
  const bar = fig.querySelector('.drawing-steps');
  if (!bar) return;
  if (kept.key) {
    diagrams.steps.delete(kept.key);
    diagrams.steps.set(kept.key, kept.at);
    if (diagrams.steps.size > DIAGRAMS_KEPT) diagrams.steps.delete(diagrams.steps.keys().next().value);
  }
  bar.querySelector('.drawing-count').textContent = `${kept.at + 1}/${last + 1}`;
  bar.querySelector('.drawing-caption').textContent = step.caption;
  bar.querySelector('[data-step-go="-1"]').disabled = kept.at === 0;
  bar.querySelector('[data-step-go="1"]').disabled = kept.at === last;
}

function stepDrawings() {
  for (const img of [...diagrams.images]) {
    const fig = img.parentElement;
    if (!img.isConnected || !fig || fig.dataset.inked !== undefined) {
      diagrams.images.delete(img);
      continue;
    }
    if (fig.classList.contains('pending') || fig.closest('.viewer-diagram')) continue;
    diagrams.images.delete(img);
    fig.dataset.inked = '';
    const src = img.getAttribute('src') || '';
    let source = '';
    try {
      source = src.startsWith(DRAWING_URL) ? decodeURIComponent(src.slice(DRAWING_URL.length)) : '';
    } catch (e) {
      continue;
    }
    if (!source.includes('var(--') && !source.includes('data-step')) continue;
    const was = diagrams.inked.get(fig)?.source === source ? diagrams.inked.get(fig) : null;
    const variants = was ? was.variants : drawingSteps(source);
    if (!variants) continue;
    const stepped = variants.length > 1 && !fig.closest('.doc');
    const thread = fig.closest('.margin-note')?.dataset.id;
    const key = thread ? `${thread}|${source}` : '';
    diagrams.inked.set(fig, { variants, at: 0, key, source });
    fig.classList.add('inked');
    if (stepped) fig.insertAdjacentHTML('beforeend', '<figcaption class="drawing-steps"><button type="button" data-step-go="-1" aria-label="Previous step">‹</button><span class="drawing-count"></span><button type="button" data-step-go="1" aria-label="Next step">›</button><button type="button" data-step-go="0" aria-label="Start again">↻</button><span class="drawing-caption" aria-live="polite"></span></figcaption>');
    const play = stepped && 'play' in fig.dataset && !(key && diagrams.steps.has(key));
    delete fig.dataset.play;
    paintDrawing(fig, stepped ? (was ? was.at : (key && diagrams.steps.get(key)) || 0) : variants.length - 1);
    if (play) playDrawing(fig);
  }
}

function playDrawing(fig) {
  const kept = diagrams.inked.get(fig);
  for (const timer of kept.timers || []) clearTimeout(timer);
  kept.timers = [];
  paintDrawing(fig, 0);
  if (stillMotion()) return paintDrawing(fig, kept.variants.length - 1);
  let wait = 0;
  for (let at = 1; at < kept.variants.length; at++) {
    wait += STEP_MS + kept.variants[at - 1].caption.length * STEP_CHAR_MS;
    kept.timers.push(setTimeout(() => fig.isConnected && paintDrawing(fig, at), wait));
  }
}

document.addEventListener('click', (e) => {
  const go = e.target.closest?.('[data-step-go]');
  const fig = go?.closest('figure.drawing');
  const kept = fig && diagrams.inked.get(fig);
  if (!kept) return;
  const step = Number(go.dataset.stepGo);
  for (const timer of kept.timers || []) clearTimeout(timer);
  kept.timers = [];
  if (step) paintDrawing(fig, kept.at + step);
  else playDrawing(fig);
});

function diagramScheme() {
  return window.leoTheme.dark() ? 'dark' : 'default';
}

function diagramTheme(dark) {
  const css = getComputedStyle(document.documentElement);
  const token = (name, fallback) => css.getPropertyValue(name).trim() || fallback;
  const accent = token('--accent', '#3f6d97');
  const soft = token('--accent-soft', '#eceafd');
  const text = token('--text', '#1c1c1a');
  const muted = token('--muted', '#6b6a65');
  const surface = token('--surface', '#ffffff');
  const surface2 = token('--surface-2', '#f0efec');
  const line = token('--line', '#e6e5e1');
  const tones = ['--chart-1', '--chart-2', '--chart-3', '--chart-4', '--chart-5', '--hue-6', '--hue-7'].map((t) => token(t, accent));
  const pies = Object.fromEntries(tones.map((tone, i) => [`pie${i + 1}`, tone]));
  return {
    darkMode: dark,
    background: surface,
    fontFamily: getComputedStyle(document.body).fontFamily,
    fontSize: '14px',
    primaryColor: soft,
    primaryBorderColor: accent,
    primaryTextColor: text,
    secondaryColor: surface2,
    secondaryBorderColor: line,
    secondaryTextColor: text,
    tertiaryColor: surface,
    tertiaryBorderColor: line,
    tertiaryTextColor: text,
    lineColor: muted,
    textColor: text,
    mainBkg: soft,
    nodeBorder: accent,
    clusterBkg: surface2,
    clusterBorder: line,
    edgeLabelBackground: surface,
    titleColor: text,
    noteBkgColor: surface2,
    noteTextColor: text,
    noteBorderColor: line,
    actorBkg: soft,
    actorBorder: accent,
    actorTextColor: text,
    signalColor: muted,
    signalTextColor: text,
    pieTitleTextSize: '17px',
    pieSectionTextColor: dark ? '#181715' : '#ffffff',
    pieSectionTextSize: '13px',
    pieLegendTextColor: text,
    pieStrokeColor: surface,
    pieOuterStrokeColor: line,
    pieOpacity: '0.9',
    ...pies,
  };
}

function loadMermaid() {
  if (window.mermaid) return Promise.resolve(window.mermaid);
  if (!diagrams.loading) {
    diagrams.loading = new Promise((resolve, reject) => {
      const script = document.createElement('script');
      script.src = MERMAID_URL;
      script.onload = () => (window.mermaid ? resolve(window.mermaid) : reject(new Error('the diagram drawer did not start')));
      script.onerror = () => {
        diagrams.loading = null;
        reject(new Error('the diagram drawer could not be loaded'));
      };
      document.head.appendChild(script);
    });
  }
  return diagrams.loading;
}

function noteDiagrams(node) {
  if (node.matches(DIAGRAM_WAITING)) diagrams.waiting.add(node);
  for (const fig of node.querySelectorAll(DIAGRAM_WAITING)) diagrams.waiting.add(fig);
  if (node.matches(DRAWING_IMAGE)) diagrams.images.add(node);
  for (const img of node.querySelectorAll(DRAWING_IMAGE)) diagrams.images.add(img);
}

function waitingDiagrams() {
  const found = [];
  for (const fig of diagrams.waiting) {
    if (!fig.isConnected || fig.dataset.drawn !== undefined) diagrams.waiting.delete(fig);
    else if (!fig.closest('.msg.pending, .viewer-diagram')) found.push(fig);
  }
  return found;
}

function bigSvg(svg) {
  const copy = svg.cloneNode(true);
  const suffix = `-big${++diagrams.seq}`;
  const ids = new Map([copy, ...copy.querySelectorAll('[id]')].filter((el) => el.id).map((el) => [el.id, el.id + suffix]));
  const swap = (text) => text.replace(/#([\w-]+)/g, (all, id) => (ids.has(id) ? `#${ids.get(id)}` : all));
  for (const el of [copy, ...copy.querySelectorAll('*')]) {
    for (const { name, value } of [...el.attributes]) {
      if (name === 'id') el.setAttribute('id', ids.get(value) || value);
      else if (value.includes('#')) el.setAttribute(name, swap(value));
    }
    if (el.localName === 'style') el.textContent = swap(el.textContent);
  }
  copy.removeAttribute('style');
  const box = copy.viewBox.baseVal;
  if (box && box.width) {
    copy.setAttribute('width', String(box.width));
    copy.setAttribute('height', String(box.height));
  }
  return copy;
}

function expandDiagrams(button) {
  const scope = button.closest('.margin-note, .msg.leo');
  if (!scope) return;
  const from = scope.matches('.margin-note') ? 'margin' : 'chat';
  const zone = from === 'margin' ? '.margin-felix' : ':scope > .prose';
  const figures = [...scope.querySelectorAll(`${zone} figure.diagram[data-drawn="yes"], ${zone} figure.drawing:not(.pending)`)];
  if (!figures.length) return;
  const items = figures.map((fig) => {
    const live = fig.querySelector('svg, img');
    const rect = live.getBoundingClientRect();
    const row = fig.closest('.drawing-row');
    const copy = (row || fig).cloneNode(true);
    copy.querySelector('.diagram-src')?.remove();
    copy.querySelector('.drawing-steps')?.remove();
    const kept = diagrams.inked.get(fig);
    if (kept) copy.querySelector('img').src = inkedUrl(kept.variants[kept.variants.length - 1].svg);
    const mine = copy.querySelector('svg, img');
    let target = mine;
    if (mine.localName === 'svg') {
      target = bigSvg(live);
      mine.replaceWith(target);
    }
    const box = target.viewBox?.baseVal;
    const wrap = document.createElement('div');
    wrap.className = `viewer-diagram from-${from}`;
    wrap.append(copy);
    return { wrap, target, row: Boolean(row), w: rect.width || box?.width || 300, h: rect.height || box?.height || 200 };
  });
  const scrim = viewPicture({ title: 'Diagram', nodes: items.map((item) => item.wrap) });
  scrim.classList.add('scrim-wide');
  const pan = scrim.querySelector('.viewer-pan');
  const room = { w: pan.clientWidth - 16, h: pan.clientHeight - 32 };
  for (const { target, row, w, h } of items) {
    const fit = Math.min(2, room.w / w, room.h / h);
    target.style.width = `calc(var(--zoom, 1) * ${row ? 180 : Math.max(40, Math.round(w * fit))}px)`;
    target.style.height = 'auto';
  }
  const reply = button.closest('.margin-felix, .msg.leo');
  const at = figures.findIndex((fig) => reply?.contains(fig));
  if (at > 0) pan.scrollTop += items[at].wrap.getBoundingClientRect().top - pan.getBoundingClientRect().top - 16;
}
const FELIX_LOOK = '%%{init: {"themeVariables": {"fontFamily": "\\"Iowan Old Style\\", \\"New York\\", Georgia, serif"}, "flowchart": {"curve": "basis", "nodeSpacing": 40, "rankSpacing": 50, "padding": 12}}}%%';
const diagramSource = (fig) => {
  const code = fig.querySelector('.diagram-src code');
  const source = code ? code.textContent : '';
  if (!source || !fig.closest('.margin-note, .msg.leo > .prose')) return source;
  const front = /^\s*---[ \t]*\n[\s\S]*?\n---[ \t]*(?:\n|$)/.exec(source);
  const at = front ? front[0].length : 0;
  return `${source.slice(0, at)}${at && !front[0].endsWith('\n') ? '\n' : ''}${FELIX_LOOK}\n${source.slice(at)}`;
};

function showDiagram(fig, svg) {
  // One picture per figure, whichever pass gets there first or last.
  for (const old of fig.querySelectorAll('.diagram-art, .diagram-error')) old.remove();
  const art = document.createElement('div');
  art.className = 'diagram-art';
  art.innerHTML = svg;
  fig.appendChild(art);
  fig.dataset.drawn = 'yes';
}

function failDiagram(fig, problem) {
  fig.dataset.drawn = 'failed';
  const note = document.createElement('figcaption');
  note.className = 'diagram-error';
  note.textContent = `This diagram could not be drawn: ${problem}`;
  fig.appendChild(note);
}

function keepDiagram(key, svg) {
  diagrams.drawn.delete(key);
  diagrams.drawn.set(key, svg);
  while (diagrams.drawn.size > DIAGRAMS_KEPT) diagrams.drawn.delete(diagrams.drawn.keys().next().value);
}

function drawKnownDiagrams() {
  const scheme = diagramScheme();
  for (const fig of waitingDiagrams()) {
    const svg = diagrams.drawn.get(`${scheme}\n${diagramSource(fig)}`);
    if (svg) showDiagram(fig, svg);
  }
}

async function drawDiagrams() {
  if (!waitingDiagrams().length) return;
  let mermaid;
  try {
    mermaid = await loadMermaid();
  } catch (e) {
    for (const fig of waitingDiagrams()) failDiagram(fig, e.message);
    return;
  }
  const scheme = diagramScheme();
  if (diagrams.scheme !== scheme) {
    mermaid.initialize({ startOnLoad: false, securityLevel: 'strict', secure: ['secure', 'securityLevel', 'startOnLoad', 'maxTextSize', 'suppressErrorRendering', 'maxEdges', 'themeCSS'], theme: 'base', themeVariables: diagramTheme(scheme === 'dark') });
    diagrams.scheme = scheme;
  }
  for (const fig of waitingDiagrams()) {
    // Another pass may have claimed this figure while this one waited for
    // Mermaid to load; drawing it again would put two pictures in it.
    if (fig.dataset.drawn) continue;
    const source = diagramSource(fig);
    const key = `${scheme}\n${source}`;
    fig.dataset.drawn = 'working';
    const id = `leo-diagram-${++diagrams.seq}`;
    try {
      const { svg } = await mermaid.render(id, source);
      keepDiagram(key, svg);
      if (fig.isConnected) showDiagram(fig, svg);
    } catch (e) {
      const stray = document.getElementById(`d${id}`);
      if (stray) stray.remove();
      if (fig.isConnected) failDiagram(fig, String((e && e.message) || e).split('\n')[0].slice(0, 160));
    }
  }
}

function watchDiagrams(records) {
  if (records) {
    let added = false;
    for (const r of records) {
      if (r.type === 'attributes' && r.target.matches(DRAWING_IMAGE) && r.target.parentElement.dataset.inked === undefined) {
        added = true;
        diagrams.images.add(r.target);
      }
      for (const n of r.addedNodes) {
        if (n.nodeType !== 1) continue;
        added = true;
        noteDiagrams(n);
      }
    }
    if (!added) return;
  }
  stepDrawings();
  drawKnownDiagrams();
  clearTimeout(diagrams.timer);
  diagrams.timer = setTimeout(() => drawDiagrams().catch(() => {}), 60);
}

for (const el of document.querySelectorAll(`${DIAGRAM_WAITING}, ${DRAWING_IMAGE}`)) (el.localName === 'img' ? diagrams.images : diagrams.waiting).add(el);
new MutationObserver(watchDiagrams).observe(document.body, { childList: true, subtree: true, attributeFilter: ['src'] });
window.leoTheme.onChange(() => {
  for (const fig of document.querySelectorAll('figure.drawing.inked')) if (diagrams.inked.has(fig)) paintDrawing(fig, diagrams.inked.get(fig).at);
  for (const fig of document.querySelectorAll('figure.diagram[data-drawn]')) {
    if (fig.closest('.viewer-diagram')) continue;
    fig.removeAttribute('data-drawn');
    diagrams.waiting.add(fig);
    for (const old of fig.querySelectorAll('.diagram-art, .diagram-error')) old.remove();
  }
  watchDiagrams();
});
