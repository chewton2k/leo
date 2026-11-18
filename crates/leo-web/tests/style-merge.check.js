const fs = require('node:fs');
const path = require('node:path');
const { execFileSync } = require('node:child_process');

const root = path.resolve(__dirname, '../../..');
const stylesDir = 'crates/leo-web/src/web/styles';
const darkFile = 'crates/leo-web/src/web/dark-tokens.css';

function skipString(css, i) {
  const q = css[i];
  i += 1;
  while (i < css.length && css[i] !== q) i += css[i] === '\\' ? 2 : 1;
  return i + 1;
}

function scan(css, from, until, stops, braces) {
  let i = from;
  let paren = 0;
  let depth = 0;
  while (i < until) {
    const c = css[i];
    if (c === '"' || c === "'") {
      i = skipString(css, i);
      continue;
    }
    if (c === '/' && css[i + 1] === '*') {
      const end = css.indexOf('*/', i + 2);
      i = end < 0 ? until : end + 2;
      continue;
    }
    if (c === '(') paren += 1;
    else if (c === ')') paren -= 1;
    else if (paren <= 0) {
      if (braces && c === '{') depth += 1;
      else if (braces && c === '}' && depth > 0) depth -= 1;
      else if (depth === 0 && stops.includes(c)) return i;
    }
    i += 1;
  }
  return until;
}

function parseBlock(css, from, until) {
  const nodes = [];
  let i = from;
  while (i < until) {
    while (i < until && /\s/.test(css[i])) i += 1;
    if (i >= until) break;
    if (css[i] === '/' && css[i + 1] === '*') {
      const end = css.indexOf('*/', i + 2);
      i = end < 0 ? until : end + 2;
      continue;
    }
    const start = i;
    const brace = scan(css, i, until, ['{', ';'], false);
    if (css[brace] !== '{') {
      nodes.push({ kind: 'statement', start, end: Math.min(brace + 1, until), text: css.slice(start, brace + 1) });
      i = brace + 1;
      continue;
    }
    const head = css.slice(start, brace).trim();
    const close = scan(css, brace + 1, until, ['}'], true);
    if (close >= until) throw new Error(`unclosed block at ${start}: ${head.slice(0, 60)}`);
    const nested = head.startsWith('@') && /^@(media|supports|layer|container)\b/.test(head);
    nodes.push({
      kind: head.startsWith('@') ? 'at' : 'rule',
      head,
      selector: head,
      body: css.slice(brace + 1, close),
      bodyStart: brace + 1,
      children: nested ? parseBlock(css, brace + 1, close) : null,
      start,
      end: close + 1,
    });
    i = close + 1;
  }
  return nodes;
}

function parseCss(css) {
  const nodes = parseBlock(css, 0, css.length);
  const seen = [];
  const walk = (list) => list.forEach((n) => {
    if (n.kind === 'rule' && /[{}]/.test(n.selector)) throw new Error(`selector holds a brace: ${n.selector.slice(0, 60)}`);
    if (n.kind === 'rule') seen.push(n);
    if (n.children) walk(n.children);
  });
  walk(nodes);
  return nodes;
}

function declarations(body) {
  const out = [];
  let i = 0;
  while (i < body.length) {
    const end = scan(body, i, body.length, [';'], false);
    const text = body.slice(i, end).trim();
    i = end + 1;
    if (!text || text.startsWith('/*')) continue;
    const colon = text.indexOf(':');
    out.push([text.slice(0, colon).trim(), text.slice(colon + 1).trim()]);
  }
  return out;
}

function splitList(list) {
  const out = [];
  let from = 0;
  let depth = 0;
  for (let i = 0; i < list.length; i += 1) {
    const c = list[i];
    if (c === '"' || c === "'") i = skipString(list, i) - 1;
    else if (c === '(' || c === '[') depth += 1;
    else if (c === ')' || c === ']') depth -= 1;
    else if (c === ',' && depth === 0) {
      out.push(list.slice(from, i).trim());
      from = i + 1;
    }
  }
  out.push(list.slice(from).trim());
  return out.filter(Boolean);
}

function darkBlocks(dark) {
  return `@media (prefers-color-scheme: dark) {\n  :root:not([data-theme="light"]) {\n${dark}  }\n}\n:root[data-theme="dark"] {\n${dark}  color-scheme: dark;\n}\n:root[data-theme="light"] { color-scheme: light; }\n`;
}

function assemble(names, read) {
  const dark = read(darkFile);
  return names.map((name) => read(`${stylesDir}/${name}`) + (name === '01-tokens.css' ? darkBlocks(dark) : '')).join('');
}

function treeCss() {
  const names = fs.readdirSync(path.join(root, stylesDir)).filter((n) => n.endsWith('.css')).sort();
  return assemble(names, (file) => fs.readFileSync(path.join(root, file), 'utf8'));
}

function baseCss(ref) {
  const git = (...args) => execFileSync('git', args, { cwd: root, encoding: 'utf8', maxBuffer: 1 << 26 });
  const names = git('ls-tree', '--name-only', ref, `${stylesDir}/`).split('\n').filter((n) => n.endsWith('.css')).map((n) => path.basename(n)).sort();
  return assemble(names, (file) => git('show', `${ref}:${file}`));
}

function ruleKeys(nodes, context, out) {
  nodes.forEach((n) => {
    if (n.kind === 'rule') out.push({ context, selector: n.selector, key: `${context}|${n.selector}|${declarations(n.body).map((d) => d.join(':')).join(';')}` });
    else if (n.kind === 'at' && n.children) ruleKeys(n.children, `${context}${n.head}>`, out);
  });
  return out;
}

function changedRules(baseText, treeText) {
  const base = ruleKeys(parseCss(baseText), '', []);
  const tree = ruleKeys(parseCss(treeText), '', []);
  const count = (list) => list.reduce((m, r) => m.set(r.key, (m.get(r.key) || 0) + 1), new Map());
  const baseCount = count(base);
  const treeCount = count(tree);
  const pick = (list, other) => {
    const left = new Map(other);
    return list.filter((r) => {
      const n = left.get(r.key) || 0;
      if (n > 0) {
        left.set(r.key, n - 1);
        return false;
      }
      return true;
    });
  };
  return [...pick(base, treeCount), ...pick(tree, baseCount)];
}

function expandIs(selector) {
  const m = /:(is|where)\(/.exec(selector);
  if (!m) return [selector];
  const open = m.index + m[0].length;
  let depth = 1;
  let i = open;
  while (i < selector.length && depth > 0) {
    if (selector[i] === '(') depth += 1;
    else if (selector[i] === ')') depth -= 1;
    i += 1;
  }
  const inner = selector.slice(open, i - 1);
  const head = selector.slice(0, m.index);
  const tail = selector.slice(i);
  return splitList(inner).flatMap((alt) => expandIs(head + alt + tail));
}

const DYNAMIC = new Set(['hover', 'focus', 'focus-visible', 'focus-within', 'active', 'visited', 'target']);

function readName(text, i) {
  let j = i;
  while (j < text.length && (/[\w-]/.test(text[j]) || text[j] === '\\' || text.charCodeAt(j) > 127)) j += text[j] === '\\' ? 2 : 1;
  return [text.slice(i, j).replace(/\\(.)/g, '$1'), j];
}

function readGroup(text, i, open, close) {
  let depth = 0;
  let j = i;
  for (; j < text.length; j += 1) {
    if (text[j] === '"' || text[j] === "'") j = skipString(text, j) - 1;
    else if (text[j] === open) depth += 1;
    else if (text[j] === close) {
      depth -= 1;
      if (depth === 0) break;
    }
  }
  return [text.slice(i + 1, j), j + 1];
}

function parseCompound(text, i) {
  const c = { tag: '', ids: [], classes: [], attrs: [], pseudos: [], element: '' };
  while (i < text.length && !/[\s>+~]/.test(text[i])) {
    const ch = text[i];
    if (ch === '.') {
      const [name, j] = readName(text, i + 1);
      c.classes.push(name);
      i = j;
    } else if (ch === '#') {
      const [name, j] = readName(text, i + 1);
      c.ids.push(name);
      i = j;
    } else if (ch === '[') {
      const [inner, j] = readGroup(text, i, '[', ']');
      const m = /^\s*([\w-]+)\s*(?:([~|^$*]?=)\s*("([^"]*)"|'([^']*)'|[^\s\]]+)(?:\s+[is])?)?\s*$/.exec(inner);
      if (!m) throw new Error(`attribute ${inner}`);
      c.attrs.push({ name: m[1], op: m[2] || '', value: m[4] ?? m[5] ?? m[3] ?? '' });
      i = j;
    } else if (ch === ':') {
      if (text[i + 1] === ':') {
        const [name, j] = readName(text, i + 2);
        c.element = name;
        i = j;
      } else {
        const [name, j] = readName(text, i + 1);
        let arg = null;
        let k = j;
        if (text[j] === '(') [arg, k] = readGroup(text, j, '(', ')');
        c.pseudos.push({ name, arg });
        i = k;
      }
    } else if (ch === '*') i += 1;
    else {
      const [name, j] = readName(text, i);
      if (!name) throw new Error(`cannot read ${text.slice(i, i + 12)}`);
      c.tag = name;
      i = j;
    }
  }
  return [c, i];
}

function parseComplex(text) {
  const parts = [];
  let i = 0;
  let combinator = ' ';
  while (i < text.length) {
    while (i < text.length && /\s/.test(text[i])) i += 1;
    if (i >= text.length) break;
    if (/[>+~]/.test(text[i])) {
      combinator = text[i];
      i += 1;
      continue;
    }
    const [compound, j] = parseCompound(text, i);
    parts.push({ combinator: parts.length ? combinator : '', compound });
    combinator = ' ';
    i = j;
  }
  return parts;
}

const VOID = new Set(['input', 'img', 'br', 'hr']);

function build(parts, out, parent) {
  let subject = null;
  let previous = null;
  parts.forEach((part, index) => {
    const { compound: c, combinator } = part;
    if (['html', 'body'].includes(c.tag) || c.pseudos.some((p) => p.name === 'root')) throw new Error('root element');
    let host = parent;
    let siblingOf = null;
    if (index > 0 && (combinator === '+' || combinator === '~')) siblingOf = previous;
    const el = { tag: c.tag || 'div', attrs: { class: c.classes.join(' ') }, children: [], forced: [], n: out.counter++ };
    if (c.ids[0]) el.attrs.id = c.ids[0];
    c.attrs.forEach((a) => {
      el.attrs[a.name] = a.op ? a.value : '';
    });
    const needs = [];
    c.pseudos.forEach((p) => {
      if (DYNAMIC.has(p.name)) needs.push(p.name);
      else if (p.name === 'checked') {
        el.tag = 'input';
        el.attrs.type = 'checkbox';
        el.attrs.checked = '';
      } else if (p.name === 'disabled') el.attrs.disabled = '';
      else if (p.name === 'placeholder-shown') el.attrs.placeholder = ' ';
      else if (p.name === 'has' && p.arg) {
        splitList(p.arg).forEach((arg) => {
          const rel = parseComplex(arg.replace(/^\s*[>+~]\s*/, ''));
          build(rel, out, el);
        });
      }
    });
    el.forced = needs;
    if (siblingOf) {
      siblingOf.sibling = siblingOf.sibling || [];
      siblingOf.sibling.push(el);
    } else if (index === 0) {
      (host || out.stage).children.push(el);
    } else {
      previous.children.push(el);
    }
    previous = el;
    subject = el;
    out.all.push(el);
    if (c.element) subject.element = c.element;
  });
  return subject;
}

function html(el) {
  const attrs = Object.entries(el.attrs).filter(([k, v]) => k !== 'class' || v).map(([k, v]) => ` ${k}="${String(v).replace(/"/g, '&quot;')}"`).join('');
  const inner = el.children.map(html).join('');
  const own = VOID.has(el.tag) ? `<${el.tag}${attrs}>` : `<${el.tag}${attrs} data-n="${el.n}">${inner}</${el.tag}>`;
  const withN = VOID.has(el.tag) ? own.replace('>', ` data-n="${el.n}">`) : own;
  return withN + (el.sibling || []).map(html).join('');
}

function stripElement(selector) {
  return selector.replace(/::[\w-]+(\([^)]*\))?/g, '');
}

function draw(rules) {
  const out = { counter: 0, all: [], stage: { children: [] } };
  const drawn = [];
  const undrawn = [];
  const seen = new Set();
  rules.forEach((rule) => {
    splitList(rule.selector).forEach((selector) => {
      const id = `${rule.context}${selector}`;
      if (seen.has(id)) return;
      seen.add(id);
      expandIs(selector).forEach((variant) => {
        if (/^@|^\d/.test(variant)) {
          undrawn.push({ selector: variant, reason: 'at-rule or keyframe' });
          return;
        }
        try {
          const parts = parseComplex(stripElement(variant));
          const mark = out.stage.children.length;
          const before = out.all.length;
          const subject = build(parts, out, null);
          const pe = /::[\w-]+(\([^)]*\))?/.exec(variant);
          drawn.push({ selector: variant, original: selector, context: rule.context, n: subject.n, pseudoElement: pe ? pe[0] : '', mark, before });
        } catch (error) {
          undrawn.push({ selector: variant, reason: error.message });
        }
      });
    });
  });
  const stage = out.stage.children.map(html).join('\n');
  return { stage, drawn, undrawn, all: out.all };
}

function hashSource() {
  return `
    const standard = Array.from(getComputedStyle(document.documentElement)).filter((p) => !p.startsWith('--')).sort();
    window.__custom = (el) => Array.from(getComputedStyle(el)).filter((p) => p.startsWith('--')).sort();
    let names = standard;
    window.__names = (extra) => { names = standard.concat(extra); };
    window.__read = (list) => {
      const result = {};
      list.forEach(([n, pe]) => {
        const el = document.querySelector('[data-n="' + n + '"]');
        if (!el) { result[n + pe] = ['missing']; return; }
        const cs = getComputedStyle(el, pe || null);
        result[n + pe] = names.map((p) => p + ':' + cs.getPropertyValue(p));
      });
      return result;
    };
    window.__settle = () => {
      document.getAnimations().forEach((a) => {
        try { if (a.transitionProperty) a.finish(); else a.cancel(); } catch (e) { a.cancel(); }
      });
    };
  `;
}

function loadPlaywright() {
  const candidates = [process.env.LEO_PLAYWRIGHT, path.join(__dirname, 'browser/node_modules/@playwright/test'), path.join(root, 'crates/leo-web/node_modules/@playwright/test'), '@playwright/test'].filter(Boolean);
  for (const candidate of candidates) {
    try {
      return require(candidate);
    } catch (error) {
      if (error.code !== 'MODULE_NOT_FOUND') throw error;
    }
  }
  throw new Error('Playwright library not found: set LEO_PLAYWRIGHT to its @playwright/test folder');
}

const PASSES = [
  { name: 'light', colorScheme: 'light', forcedColors: 'none', width: 1280 },
  { name: 'dark', colorScheme: 'dark', forcedColors: 'none', width: 1280 },
  { name: 'light narrow', colorScheme: 'light', forcedColors: 'none', width: 390 },
  { name: 'dark narrow', colorScheme: 'dark', forcedColors: 'none', width: 390 },
  { name: 'light forced colours', colorScheme: 'light', forcedColors: 'active', width: 1280 },
  { name: 'dark forced colours', colorScheme: 'dark', forcedColors: 'active', width: 1280 },
];

async function readPasses(browser, css, stage, targets, forcedList, custom) {
  const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
  try {
    await page.setContent(`<!doctype html><html lang="en"><head><meta charset="utf-8"><style>${css}</style></head><body><div id="stage">${stage}</div></body></html>`);
    await page.evaluate(hashSource());
    const customNames = custom || (await page.evaluate(() => window.__custom(document.documentElement)));
    await page.evaluate((extra) => window.__names(extra), customNames);
    const client = await page.context().newCDPSession(page);
    await client.send('DOM.enable');
    await client.send('CSS.enable');
    const { root: doc } = await client.send('DOM.getDocument', { depth: 0 });
    const results = {};
    for (const pass of PASSES) {
      await page.setViewportSize({ width: pass.width, height: 900 });
      await page.emulateMedia({ colorScheme: pass.colorScheme, forcedColors: pass.forcedColors });
      for (const [n, states] of forcedList) {
        const { nodeId } = await client.send('DOM.querySelector', { nodeId: doc.nodeId, selector: `[data-n="${n}"]` });
        await client.send('CSS.forcePseudoState', { nodeId, forcedPseudoClasses: states });
      }
      await page.evaluate('window.__settle()');
      await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
      await page.evaluate('window.__settle()');
      results[pass.name] = await page.evaluate((list) => window.__read(list), targets);
    }
    const matches = await page.evaluate((list) => Object.fromEntries(list.map(([n, selector]) => {
      const el = document.querySelector('[data-n="' + n + '"]');
      try { return [n, el ? el.matches(selector) : false]; } catch (e) { return [n, false]; }
    })), [...selectorOf]);
    return { results, matches, custom: customNames };
  } finally {
    await page.close();
  }
}

const selectorOf = new Map();

async function compare(baseText, treeText, log = () => {}) {
  const changed = changedRules(baseText, treeText);
  const { stage, drawn, undrawn, all } = draw(changed);
  const forced = new Map();
  all.forEach((el) => {
    if (el.forced.length) forced.set(el.n, [...new Set(el.forced)]);
  });
  drawn.forEach((d) => selectorOf.set(d.n, stripElement(d.selector)));
  const forcedList = [...forced];
  const targets = [];
  const seenTarget = new Set();
  const add = (n, pe) => {
    if (!seenTarget.has(n + pe)) {
      seenTarget.add(n + pe);
      targets.push([n, pe]);
    }
  };
  all.forEach((el) => ['', '::before', '::after'].forEach((pe) => add(el.n, pe)));
  drawn.forEach((d) => d.pseudoElement && add(d.n, d.pseudoElement));
  const { chromium } = loadPlaywright();
  const browser = await chromium.launch();
  const differences = [];
  let undrawnMatches = [];
  try {
    const base = await readPasses(browser, baseText, stage, targets, forcedList);
    const tree = await readPasses(browser, treeText, stage, targets, forcedList, base.custom);
    const matching = new Map(Object.entries(base.matches).map(([n, ok]) => [Number(n), ok]));
    PASSES.forEach((pass) => {
      Object.keys(base.results[pass.name]).forEach((k) => {
        const a = base.results[pass.name][k];
        const b = tree.results[pass.name][k];
        const props = a.map((p, i) => (p === b[i] ? null : `${p} -> ${b[i]}`)).filter(Boolean);
        if (!props.length) return;
        const d = drawn.find((x) => String(x.n) + (x.pseudoElement || '') === k) || drawn.find((x) => String(x.n) === k.replace(/::.*$/, ''));
        differences.push({ pass: pass.name, element: k, selector: d ? d.selector : '(inside a drawn chain)', props });
      });
    });
    undrawnMatches = drawn.filter((d) => !matching.get(d.n)).map((d) => ({ selector: d.selector, reason: 'drawn element does not match its selector' }));
  } finally {
    await browser.close();
  }
  const drawnOk = drawn.length - undrawnMatches.length;
  return { changed: changed.length, drawn: drawnOk, left: [...undrawn, ...undrawnMatches], differences, elements: all.length };
}

async function main() {
  const ref = process.argv[2] || process.env.LEO_STYLE_BASE || 'origin/overhaul-main';
  const base = baseCss(ref);
  const tree = treeCss();
  const result = await compare(base, tree);
  console.log(`base ${ref}: ${Buffer.byteLength(base)} bytes; tree: ${Buffer.byteLength(tree)} bytes`);
  console.log(`rules changed: ${result.changed}; selectors drawn: ${result.drawn}; elements: ${result.elements}; passes: ${PASSES.map((p) => p.name).join(', ')}`);
  console.log(`selectors left alone (not drawn): ${result.left.length}`);
  result.left.forEach((l) => console.log(`  left: ${l.selector}  [${l.reason}]`));
  result.differences.forEach((d) => console.log(`DIFF ${d.pass} ${d.selector}\n    ${d.props.join('\n    ')}`));
  console.log(`computed-style differences: ${result.differences.length}`);
  process.exitCode = result.differences.length ? 1 : 0;
}

module.exports = { parseCss, parseComplex, declarations, splitList, assemble, treeCss, baseCss, compare, changedRules, expandIs };

if (require.main === module) main().catch((error) => {
  console.error(error);
  process.exit(2);
});
