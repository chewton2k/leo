const assert = require('node:assert/strict');
const { test } = require('node:test');
const { pageCss } = require('./page-source.js');

function rules(css) {
  css = css.replace(/\/\*[\s\S]*?\*\//g, '');
  const out = [];
  const walk = (text, at) => {
    let i = 0;
    while (i < text.length) {
      const open = text.indexOf('{', i);
      if (open < 0) break;
      const head = text.slice(i, open).trim();
      let depth = 1;
      let j = open + 1;
      while (depth && j < text.length) {
        if (text[j] === '{') depth++;
        else if (text[j] === '}') depth--;
        j++;
      }
      const body = text.slice(open + 1, j - 1);
      if (head.startsWith('@')) walk(body, at.concat(head));
      else if (body.includes('{')) walk(body, at);
      else out.push({ selector: head, body, at });
      i = j;
    }
  };
  walk(css, []);
  return out;
}

const declarations = (body) => body.split(';').map((d) => d.trim()).filter(Boolean).map((d) => {
  const at = d.indexOf(':');
  return [d.slice(0, at).trim(), d.slice(at + 1).trim()];
});
const all = rules(pageCss());
const themes = {
  light: all.find((r) => r.selector === ':root' && r.at.length === 0 && r.body.includes('--text:')),
  'dark media': all.find((r) => r.selector === ':root:not([data-theme="light"])' && r.at.some((a) => a.includes('prefers-color-scheme: dark'))),
  'dark theme': all.find((r) => r.selector === ':root[data-theme="dark"]' && r.body.includes('--text:')),
};
const tokens = (rule) => Object.fromEntries(declarations(rule.body).filter(([k]) => k.startsWith('--')));

function rgb(value) {
  const hex = /^#([0-9a-f]{6})$/i.exec(value);
  if (hex) return [0, 2, 4].map((i) => parseInt(hex[1].slice(i, i + 2), 16)).concat(1);
  const fn = /^rgba?\(([^)]+)\)$/.exec(value);
  assert.ok(fn, `not a hex or rgba colour: ${value}`);
  const p = fn[1].split(',').map(Number);
  return [p[0], p[1], p[2], p.length > 3 ? p[3] : 1];
}
const over = (top, under) => [0, 1, 2].map((i) => top[i] * top[3] + under[i] * (1 - top[3])).concat(1);
const luminance = (c) => {
  const [r, g, b] = c.slice(0, 3).map((v) => {
    v /= 255;
    return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
};
const contrast = (a, b) => {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
};

test('every theme block defines the new focus, scrim and wash tokens', () => {
  for (const [name, rule] of Object.entries(themes)) {
    assert.ok(rule, `${name} block found`);
    const t = tokens(rule);
    for (const key of ['--focus', '--scrim', '--wash', '--accent', '--accent-soft', '--accent-text']) assert.ok(t[key], `${name} defines ${key}`);
  }
});

test('the remapped tokens are plain hex or rgba, never color-mix, so Mermaid can read them', () => {
  for (const [name, rule] of Object.entries(themes)) {
    const t = tokens(rule);
    for (const key of ['--accent', '--accent-soft', '--accent-text', '--wash', '--focus', '--scrim', '--line', '--shadow', '--shadow-sm', '--shadow-pop']) {
      assert.ok(t[key], `${name} defines ${key}`);
      assert.doesNotMatch(t[key], /color-mix|var\(/, `${name} ${key}`);
    }
    for (const key of ['--accent', '--accent-soft', '--accent-text', '--wash', '--focus', '--scrim', '--line']) rgb(t[key]);
  }
});

test('ink, focus and highlight pairs keep their WCAG contrast in every theme', () => {
  for (const [name, rule] of Object.entries(themes)) {
    const t = tokens(rule);
    const page = rgb(t['--page']);
    const ground = rgb(t['--ground']);
    for (const ink of ['--text', '--muted', '--faint']) {
      for (const [where, under] of [['page', page], ['ground', ground]]) {
        assert.ok(contrast(rgb(t[ink]), under) >= 4.5, `${name}: ${ink} on ${where} is ${contrast(rgb(t[ink]), under).toFixed(2)}`);
      }
    }
    assert.ok(contrast(rgb(t['--focus']), page) >= 3, `${name}: focus on page`);
    assert.ok(contrast(rgb(t['--accent-text']), rgb(t['--accent'])) >= 4.5, `${name}: accent-text on accent`);
    assert.ok(contrast(rgb(t['--accent-text']), rgb(t['--focus'])) >= 4.5, `${name}: search highlight text`);
    assert.ok(contrast(rgb(t['--text']), over(rgb(t['--accent-soft']), page)) >= 4.5, `${name}: text on a selected row`);
  }
  const highlight = all.find((r) => r.selector === '::highlight(leo-search)');
  assert.match(highlight.body, /background: var\(--focus\)/);
  assert.match(highlight.body, /color: var\(--accent-text\)/);
});

test('every focus cue that sets an outline, border colour or ring uses the focus token', () => {
  const ok = (value) => /var\(--focus\)/.test(value) || /^(none|0)$/.test(value) || (/var\(--accent-soft\)/.test(value) && !/var\(--accent\)/.test(value));
  const checked = [];
  for (const rule of all) {
    if (rule.at.some((a) => a.includes('forced-colors'))) continue;
    const parts = rule.selector.split(/,(?![^(]*\))/).map((p) => p.trim());
    if (!parts.some((p) => p.includes(':focus'))) continue;
    for (const [key, value] of declarations(rule.body)) {
      if (!/^(outline|outline-color|border-color|box-shadow)$/.test(key)) continue;
      checked.push(rule.selector);
      assert.ok(ok(value), `${rule.selector} { ${key}: ${value} }`);
      if (/var\(--focus\)/.test(value)) assert.ok(parts.every((p) => p.includes(':focus')), `${rule.selector} gives a non-focus state the focus colour`);
    }
  }
  assert.ok(checked.length >= 60, `walked ${checked.length} focus declarations`);
  for (const selector of ['.inline-edit', '.settings-section [data-find].found-still', '.cal-compose']) {
    assert.match(all.find((r) => r.selector === selector).body, /var\(--focus\)/, selector);
  }
});

test('links are ink and underlined, toasts sit on paper, and scrims use the token', () => {
  const body = (selector) => all.find((r) => r.selector === selector)?.body || '';
  assert.match(body('.link-btn'), /text-decoration: underline/);
  for (const r of all.filter((r) => /(^|[\s,])a(?=$|[\s,:])|\.link-btn|\.set-link/.test(r.selector))) assert.doesNotMatch(r.body, /text-decoration: none/, r.selector);
  const toast = body('.toast, .toast-gone');
  assert.match(toast, /background: var\(--raised\)/);
  assert.match(toast, /color: var\(--text\)/);
  assert.match(toast, /box-shadow: var\(--shadow-pop\)/);
  assert.match(body('.toast button, .toast-gone button'), /text-decoration: underline/);
  assert.match(body('.toast.bad, .toast-gone.bad'), /background: var\(--danger\)/);
  assert.match(body('.scrim, .scrim-gone'), /background: var\(--scrim\)/);
  assert.doesNotMatch(pageCss(), /rgba\(0, 0, 0, \.35\);/);
  const forced = all.find((r) => r.at.some((a) => a.includes('forced-colors')) && r.selector.includes('.toast') && /CanvasText/.test(r.body));
  for (const surface of ['.popup-menu', '.home-ask', '.side-filter', '.chat-compose', '.toast']) assert.ok(forced.selector.split(/,\s*/).includes(surface), surface);
  const print = all.find((r) => r.selector === ':root:root' && r.at.includes('@media print'));
  assert.match(print.body, /--accent: #1c1c1a/);
  assert.match(print.body, /--accent-text: #fff/);
});

test('sizes and radii come from the scale, and the note title and prose use theirs', () => {
  for (const rule of all) {
    for (const [key, value] of declarations(rule.body)) {
      if (key === 'font-size' || key === 'font') assert.doesNotMatch(value, /^(11|11\.5|12\.5|13\.5|14\.5|15\.5)px\b/, `${rule.selector} { ${key}: ${value} }`);
      if (/^border(-[a-z]+)*-radius$/.test(key)) assert.doesNotMatch(value, /(^|[\s(])(2|3|7|9|12)px\b/, `${rule.selector} { ${key}: ${value} }`);
    }
  }
  const t = tokens(themes.light);
  assert.equal(t['--fs-title'], '32px');
  assert.equal(t['--r-xl'], '14px');
  const body = (selector) => all.find((r) => r.selector === selector && r.at.length === 0)?.body || '';
  assert.match(body('.note-head h1'), /font-size: var\(--fs-title\); font-weight: 650;[^}]*letter-spacing: -\.025em/);
  assert.match(body('.prose'), /line-height: 1\.72/);
  assert.match(body('.doc .line-edit'), /line-height: 1\.72/);
});

test('the explorer and Home rest quietly: faint sentence-case labels, a borderless filter, a shelf-toned quick bar', () => {
  const body = (selector) => all.find((r) => r.selector === selector && r.at.length === 0)?.body || '';
  const label = all.find((r) => r.selector.split(/,\s*/).includes('.side-group') && /font-size: var\(--fs-xs\)/.test(r.body));
  assert.match(label.body, /color: var\(--faint\)/);
  assert.doesNotMatch(label.body, /text-transform|letter-spacing/);
  assert.match(body('.side-filter'), /border-color: transparent/);
  assert.match(body('.side-row.on'), /background: var\(--accent-soft\)[^}]*font-weight: 550/);
  assert.match(body('.side-row:hover'), /background: var\(--wash\)/);
  assert.doesNotMatch(body('.side-new kbd'), /border|padding/);
  const ask = body('.home-ask');
  assert.match(ask, /background: var\(--ground\)/);
  assert.doesNotMatch(ask, /box-shadow|var\(--line\)/);
  assert.match(body('.home-ask input'), /font-size: var\(--fs-lg\)/);
  assert.match(body('.empty svg'), /width: 22px/);
  assert.match(body('.empty h3'), /font-size: var\(--fs-md\); font-weight: var\(--fw-semibold\)/);
  assert.match(body('.empty p'), /font-size: var\(--fs-sm\)/);
});
