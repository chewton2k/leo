const KATEX = '/vendor/katex-0.16.11/';
const MATH_KEPT = 300;
const MATH_WAITING = '.math:not([data-drawn])';
const math = { loading: null, drawn: new Map(), timer: 0, waiting: new Set() };

function loadKatex() {
  if (window.katex) return Promise.resolve(window.katex);
  if (!math.loading) {
    math.loading = new Promise((resolve, reject) => {
      const sheet = document.createElement('link');
      sheet.rel = 'stylesheet';
      sheet.href = `${KATEX}katex.min.css`;
      document.head.appendChild(sheet);
      const script = document.createElement('script');
      script.src = `${KATEX}katex.min.js`;
      script.onload = () => (window.katex ? resolve(window.katex) : reject(new Error('the formula drawer did not start')));
      script.onerror = () => {
        math.loading = null;
        reject(new Error('the formula drawer could not be loaded'));
      };
      document.head.appendChild(script);
    });
  }
  return math.loading;
}

function noteMath(node) {
  if (node.matches(MATH_WAITING)) math.waiting.add(node);
  for (const el of node.querySelectorAll(MATH_WAITING)) math.waiting.add(el);
}
function waitingMath() {
  const found = [];
  for (const el of math.waiting) {
    if (!el.isConnected || el.dataset.drawn !== undefined) math.waiting.delete(el);
    else if (!el.closest('.msg.pending, .editing')) found.push(el);
  }
  return found;
}
const mathKey = (el) => `${el.classList.contains('math-block') ? 'block' : 'inline'}\n${el.dataset.tex || ''}`;

function placeMath(el, html) {
  el.innerHTML = html;
  el.dataset.drawn = 'yes';
}

function drawKnownMath() {
  for (const el of waitingMath()) {
    const html = math.drawn.get(mathKey(el));
    if (html) placeMath(el, html);
  }
}

async function drawMath() {
  if (!waitingMath().length) return;
  let katex;
  try {
    katex = await loadKatex();
  } catch (e) {
    for (const el of waitingMath()) el.dataset.drawn = 'failed';
    return;
  }
  for (const el of waitingMath()) {
    const key = mathKey(el);
    try {
      const html = katex.renderToString(el.dataset.tex || '', {
        displayMode: el.classList.contains('math-block'),
        throwOnError: false,
        trust: false,
        strict: 'ignore',
        output: 'htmlAndMathml',
      });
      math.drawn.set(key, html);
      if (math.drawn.size > MATH_KEPT) math.drawn.delete(math.drawn.keys().next().value);
      placeMath(el, html);
    } catch (e) {
      el.dataset.drawn = 'failed';
    }
  }
}

for (const el of document.querySelectorAll(MATH_WAITING)) math.waiting.add(el);
new MutationObserver((records) => {
  let added = false;
  for (const r of records) {
    for (const n of r.addedNodes) {
      if (n.nodeType !== 1) continue;
      added = true;
      noteMath(n);
    }
  }
  if (!added) return;
  drawKnownMath();
  clearTimeout(math.timer);
  math.timer = setTimeout(() => drawMath().catch(() => {}), 30);
}).observe(document.body, { childList: true, subtree: true });
