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
    assert.ok(contrast(rgb(t['--muted']), over(rgb(t['--accent-soft']), ground)) >= 4.5, `${name}: counts on a selected row`);
  }
  const highlight = all.find((r) => r.selector === '::highlight(leo-search)');
  assert.match(highlight.body, /background: var\(--focus\)/);
  assert.match(highlight.body, /color: var\(--accent-text\)/);
});

test('every focus cue that sets an outline, border colour or ring uses the focus token', () => {
  const ok = (value) => /var\(--focus\)/.test(value) || /^(none|0)$/.test(value) || (/var\(--(accent-soft|ring-soft)\)/.test(value) && !/var\(--accent\)/.test(value));
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
  assert.ok(checked.length >= 50, `walked ${checked.length} focus declarations`);
  for (const selector of ['.inline-edit', '.settings-section [data-find].found-still', '.cal-compose']) {
    assert.match(all.find((r) => r.selector === selector).body, /var\(--focus\)/, selector);
  }
});

test('links are ink and underlined, toasts sit on paper, and scrims use the token', () => {
  const body = (selector) => all.find((r) => r.selector === selector)?.body || '';
  assert.match(body('.link-btn'), /text-decoration: underline/);
  for (const r of all.filter((r) => /(^|[\s,])a(?=$|[\s,:])|\.link-btn|\.set-link/.test(r.selector) && r.selector !== '#doc a')) assert.doesNotMatch(r.body, /text-decoration: none/, r.selector);
  assert.match(body('#doc a'), /background: var\(--link-under\) 0 100% \/ 100% 4px no-repeat;.*box-decoration-break: clone/);
  for (const at of ['#doc a', '#doc a:nth-of-type(3n+2)', '#doc a:nth-of-type(3n)']) assert.match(all.filter((r) => r.selector === at).map((r) => r.body).join(''), /--link-under: url\("data:image\/svg\+xml/, at);
  assert.match(all.find((r) => r.selector === '#doc a' && r.at.some((a) => a.includes('forced-colors') && a.includes('print'))).body, /background: none; text-decoration: underline/);
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
  assert.match(body('.note-head h1'), /font-family: var\(--writing-font\); font-size: var\(--fs-title\); font-weight: 500;[^}]*letter-spacing: -\.032em/);
  assert.match(body('.prose'), /line-height: 1\.72/);
  assert.match(body('.doc .line-edit'), /line-height: 1\.72/);
});

test('the paper grain is a data: SVG background layer on the shell surfaces, screen only and never in forced colours or print', () => {
  for (const [name, theme] of Object.entries(themes)) assert.match(theme.body, /--grain: url\("data:image\/svg\+xml,[^"]*feTurbulence[^"]*"\)/, name);
  const used = all.filter((r) => /var\(--grain\)/.test(r.body));
  assert.deepEqual(used.map((r) => r.selector).sort(), ['body', 'body::before, body::after']);
  for (const rule of used) {
    assert.ok(rule.at.some((a) => /^@media screen\b/.test(a) && /\(forced-colors: none\)/.test(a)), rule.selector);
    assert.ok(rule.at.every((a) => !/forced-colors: active|print/.test(a)), rule.selector);
  }
  assert.doesNotMatch(pageCss(), /mix-blend-mode|background-blend-mode/);
});

test('ink details: a warm selection wash, a drawn rule with a plain fallback, and thin warm scrollbars that keep the page bar', () => {
  const at = (selector, media = null) => all.find((r) => r.selector.split(', ').includes(selector) && (media ? r.at.some((a) => a.includes(media)) : r.at.length === 0));
  assert.match(at('::selection').body, /background: color-mix\(in srgb, var\(--text\) 22%, transparent\)/);
  assert.match(at('::highlight(leo-pick)', 'forced-colors: none').body, /background: color-mix\(in srgb, var\(--text\) 22%, transparent\)/);
  const unpainted = all.find((r) => r.selector.startsWith('.doc.pick-paint') && r.at.some((x) => x.includes('forced-colors: none')));
  assert.match(unpainted.body, /background: transparent/);
  for (const [name, rule] of Object.entries(themes)) {
    const t = tokens(rule);
    const wash = over([...rgb(t['--text']).slice(0, 3), 0.22], rgb(t['--page']));
    assert.ok(contrast(rgb(t['--text']), wash) >= 4.5, `${name}: selected text is ${contrast(rgb(t['--text']), wash).toFixed(2)}`);
  }
  const rule = at('.prose hr').body;
  assert.match(rule, /mask: url\("data:image\/svg\+xml,[^"]*preserveAspectRatio='none'/);
  assert.match(rule, /background: color-mix\(in srgb, var\(--text\)/);
  assert.match(at('.prose hr', 'forced-colors: active').body, /mask: none; border-top: 1px solid var\(--line\)/);
  assert.ok(at('.prose hr', 'forced-colors: active').at.some((a) => a.includes('print')), 'print keeps the plain rule');
  assert.ok(all.some((r) => r.selector === '*' && r.at.length === 0 && /scrollbar-width: thin/.test(r.body)));
  assert.ok(all.some((r) => r.selector === ':root' && r.at.length === 0 && /scrollbar-color: color-mix\(in srgb, var\(--text\) 24%, transparent\) transparent/.test(r.body)));
  assert.match(at('html.page-bar-on').body, /scrollbar-width: none/);
});

test('Felix blue is one plain token per theme, readable on the page and ground, used only while he works', () => {
  for (const [name, rule] of Object.entries(themes)) {
    const t = tokens(rule);
    assert.match(t['--felix'] || '', /^#[0-9a-f]{6}$/i, `${name} defines --felix`);
    for (const under of ['--page', '--ground']) assert.ok(contrast(rgb(t['--felix']), rgb(t[under])) >= 4.5, `${name}: --felix on ${under} is ${contrast(rgb(t['--felix']), rgb(t[under])).toFixed(2)}`);
  }
  assert.equal(tokens(themes.light)['--felix'], '#3f6d97');
  assert.equal(tokens(themes['dark media'])['--felix'], '#b4cfe7');
  assert.equal(tokens(themes['dark theme'])['--felix'], '#b4cfe7');
  const users = all.filter((r) => /var\(--felix\)/.test(r.body)).map((r) => r.selector).sort();
  assert.deepEqual(users, [
    '.chat-btn.working::after',
    '.margin-ink path.pencil',
    '.margin-ink path.solid[data-id="draft"], .margin-ink path.tether[data-id="draft"]',
    '.margin-ink path.wash[data-id="draft"]',
    '.margin-note[data-id="draft"] .margin-reply',
    '.margin-waiting .margin-drop, .margin-mark.reading .margin-drop',
    '.msg-step-busy',
    '.msg-who',
    '.msg.pending > .prose:not(:has(.typing)):not(:has(~ .prose)) > :last-child:is(p, h1, h2, h3, h4, h5, h6)::after, .msg.pending > .prose:not(:has(.typing)):not(:has(~ .prose)) > :last-child:is(ul, ol) > li:last-child::after',
    '.proposal-wait',
    '.side-reading',
    '.study-idea.now .study-ring',
  ]);
});

test('quiz chalk tones are one plain token pair per theme, readable as text on every chat surface', () => {
  for (const [name, rule] of Object.entries(themes)) {
    const t = tokens(rule);
    for (const tone of ['--chalk-good', '--chalk-bad']) {
      assert.match(t[tone] || '', /^#[0-9a-f]{6}$/i, `${name} defines ${tone}`);
      for (const under of ['--page', '--ground', '--surface', '--surface-2', '--bg', '--raised']) assert.ok(contrast(rgb(t[tone]), rgb(t[under])) >= 4.5, `${name}: ${tone} on ${under} is ${contrast(rgb(t[tone]), rgb(t[under])).toFixed(2)}`);
      for (const under of ['--page', '--surface']) {
        const badge = over([...rgb(t[tone]).slice(0, 3), 0.14], rgb(t[under]));
        assert.ok(contrast(rgb(t[tone]), badge) >= 4.5, `${name}: ${tone} on its badge over ${under} is ${contrast(rgb(t[tone]), badge).toFixed(2)}`);
      }
    }
  }
  const at = (selector) => all.find((r) => r.selector === selector).body;
  assert.match(at('.quiz-card.right'), /var\(--chalk-good\)/);
  assert.match(at('.quiz-card.wrong'), /var\(--chalk-bad\)/);
  assert.match(at('.quiz-mark'), /mask: var\(--chalk-break\)/);
});

test('the reduced-motion rule covers pseudo-elements', () => {
  const rule = all.find((r) => r.selector.startsWith('*') && r.at.some((a) => a.includes('prefers-reduced-motion: reduce')) && /animation:\s*none\s*!important/.test(r.body));
  assert.ok(rule, 'a reduced-motion rule turns animation off');
  assert.match(rule.selector, /\*::before/);
  assert.match(rule.selector, /\*::after/);
});

test('the visually hidden rule is written once', () => {
  const css = pageCss();
  assert.equal(css.split('clip-path: inset(50%)').length, 2);
  const hidden = all.filter((r) => r.body.includes('clip-path: inset(50%)'));
  assert.equal(hidden.length, 1);
  const selectors = hidden[0].selector.split(',').map((s) => s.trim());
  assert.deepEqual(selectors.sort(), ['.chat-refs .file-reading', '.code-said', '.fa-sr', '.hc-sr', '.home-tally-said', '.kanban-priority-word', '.margin-said-aloud', '.said'].sort());
  assert.ok(declarations(hidden[0].body).some(([name, value]) => name === 'white-space' && value === 'nowrap'));
  const fixed = all.filter((r) => r.selector === '.code-said');
  assert.equal(fixed.length, 1);
  assert.deepEqual(declarations(fixed[0].body), [['position', 'fixed']]);
  assert.ok(all.indexOf(fixed[0]) > all.indexOf(hidden[0]));
});

test('the later chip rule is the only one', () => {
  const fs = require('node:fs');
  const path = require('node:path');
  const schedule = fs.readFileSync(path.join(__dirname, '../src/web/styles/10-schedule.css'), 'utf8');
  assert.ok(!/\.pick-chip/.test(schedule));
  assert.equal(all.filter((r) => r.selector === '.pick-chip').length, 1);
});

test('the dark tokens are written once and both dark blocks carry the same ones', () => {
  const fs = require('node:fs');
  const path = require('node:path');
  const source = fs.readFileSync(path.join(__dirname, '../src/web/styles/01-tokens.css'), 'utf8');
  assert.ok(!source.includes('prefers-color-scheme: dark'));
  assert.ok(!source.includes('data-theme="dark"'));
  const media = new Map(declarations(themes['dark media'].body));
  const theme = new Map(declarations(themes['dark theme'].body));
  assert.equal(theme.get('color-scheme'), 'dark');
  theme.delete('color-scheme');
  assert.ok(media.size > 40);
  assert.deepEqual([...media], [...theme]);
});

test('the focus ring is written once per offset, twice only where a rule between its members sets an outline', () => {
  const kinds = new Map();
  for (const rule of all) {
    if (rule.at.length) continue;
    const d = declarations(rule.body);
    if (!d.length || d[0][0] !== 'outline' || d[0][1] !== '2px solid var(--focus)') continue;
    if (!d.every(([key]) => ['outline', 'outline-offset', 'border-radius'].includes(key))) continue;
    const key = d.map((pair) => pair.join(': ')).join('; ');
    kinds.set(key, (kinds.get(key) || 0) + 1);
  }
  for (const [key, count] of kinds) assert.ok(count <= 2, `${key} is written ${count} times`);
  assert.ok([...kinds.values()].reduce((a, b) => a + b, 0) <= 14, 'the ring rules grew back');
});

test('the chip on-state is written once', () => {
  const on = all.filter((r) => /background: var\(--accent\); border-color: var\(--accent\); color: var\(--accent-text\)/.test(r.body) && /\.on\b/.test(r.selector));
  assert.deepEqual(on.map((r) => r.selector), ['.chat-class.on, .repeat-day.on', '.pick-chip.on']);
});
