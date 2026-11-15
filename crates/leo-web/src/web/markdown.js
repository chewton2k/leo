(function (root) {
  'use strict';

  const ESCAPES = { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' };
  const escape = (s) => s.replace(/[&<>"']/g, (c) => ESCAPES[c]);

  const safeUrl = (url) => (/^(https?:|mailto:)/i.test(url.trim()) ? url.trim() : null);

  const link = (url, label) =>
    `<a href="${escape(url)}" target="_blank" rel="noopener noreferrer">${label}</a>`;

  const SPAN_TAGS = { '*1': 'em', '*2': 'strong', '_1': 'em', '_2': 'strong', '~2': 'del' };
  const wordy = (c) => /[\p{L}\p{N}]/u.test(c);

  function spans(s) {
    const bits = s.split(/(\*+|_+|~~)/);
    const runs = [];
    const open = [];
    const beside = (i, step) => {
      for (let k = i + step; k >= 0 && k < bits.length; k += step) if (bits[k]) return step > 0 ? bits[k][0] : bits[k].at(-1);
      return '';
    };
    for (let i = 1; i < bits.length; i += 2) {
      const run = { at: i, mark: bits[i][0], left: bits[i].length, opens: '', shuts: '' };
      runs.push(run);
      const before = beside(i, -1);
      const after = beside(i, 1);
      let canOpen = /\S/.test(after);
      let canClose = /\S/.test(before);
      if (run.mark === '_') {
        canOpen = canOpen && !wordy(before);
        canClose = canClose && !wordy(after);
      }
      if (run.mark === '*' && run.left === 1 && wordy(before)) canOpen = false;
      while (canClose && run.left) {
        let k = open.length - 1;
        while (k >= 0 && open[k].mark !== run.mark) k--;
        if (k < 0) break;
        const first = open[k];
        open.length = k + 1;
        const size = Math.min(first.left, run.left) >= 2 ? 2 : 1;
        const tag = SPAN_TAGS[run.mark + size];
        first.left -= size;
        run.left -= size;
        first.opens = `<${tag}>${first.opens}`;
        run.shuts += `</${tag}>`;
        if (!first.left) open.pop();
      }
      if (canOpen && run.left) open.push(run);
    }
    for (const run of runs) bits[run.at] = run.shuts + run.mark.repeat(run.left) + run.opens;
    return bits.join('');
  }

  const emphasis = (s) => spans(s).replace(/&lt;u&gt;(.+?)&lt;\/u&gt;/g, '<u>$1</u>');

  let pictureDir = '';

  function sized(alt) {
    const found = /^(.*?)\|\s*(\d{1,4})(?:\s*x\s*\d{1,4})?\s*$/.exec(String(alt || ''));
    if (!found) return { alt: String(alt || ''), width: 0 };
    const width = Number(found[2]);
    return { alt: found[1], width: width >= 16 && width <= 4000 ? width : 0 };
  }

  function picture(target, label) {
    const { alt, width } = sized(label);
    const size = width ? ` width="${width}"` : '';
    const src = String(target || '').trim().replace(/^<(.*)>$/, '$1');
    if (/^data:image\/svg\+xml[;,][^\s"'<>]*$/i.test(src)) return `<img class="note-img drawing-img" src="${escape(src)}" alt="${escape(alt)}"${size}>`;
    if (!src || /^[a-z][a-z0-9+.-]*:/i.test(src) || src.startsWith('//')) return null;
    const url = `/api/image?path=${encodeURIComponent(src)}&from=${encodeURIComponent(pictureDir)}`;
    return `<img class="note-img" src="${escape(url)}" alt="${escape(alt)}" loading="lazy"${size}>`;
  }

  const MOST_DRAWING = 200000;

  function drawing(source) {
    const svg = source.trim();
    if (!/^(<\?xml[^>]*>\s*)?<svg[\s>]/i.test(svg) || svg.length > MOST_DRAWING) return null;
    if (!/<\/svg>$/i.test(svg)) return '<figure class="drawing pending"><span>Drawing…</span></figure>';
    const named = svg.match(/<title[^>]*>([^<]{1,200})<\/title>/i);
    const alt = named ? named[1].trim() : 'Drawing';
    const open = svg.match(/<svg[^>]*>/i);
    const ready = open && /\sxmlns=/.test(open[0]) ? svg : svg.replace(/<svg/i, '<svg xmlns="http://www.w3.org/2000/svg"');
    return `<figure class="drawing"><img src="data:image/svg+xml;charset=utf-8,${encodeURIComponent(ready)}" alt="${escape(alt)}"></figure>`;
  }

  function pairedRow(figure, source, lines, from) {
    const box = /<svg[^>]*\sviewBox\s*=\s*["']\s*[-\d.]+[\s,]+[-\d.]+[\s,]+([\d.]+)/i.exec(source);
    if (figure.includes('drawing pending') || !box || Number(box[1]) > 200) return null;
    let at = from;
    if (lines[at] !== undefined && !lines[at].trim()) at++;
    if (lines[at] === undefined || !lines[at].trim() || startsBlock(lines[at], lines[at + 1])) return null;
    const para = [];
    while (at < lines.length && lines[at].trim() && (!para.length || !startsBlock(lines[at], lines[at + 1]))) para.push(lines[at++].trim());
    const [title, sub, ...rest] = para.map((text, n) => (n < 2 ? text.replace(/^\*{1,2}(.+?)\*{1,2}$/, '$1').replace(/^\*+$/, '').trim() : text));
    const said = rest.length ? inline(rest.join(' ')) : '';
    const html = `<div class="drawing-row">${figure}<p>${title ? `<span class="drawing-title">${inline(title)}</span>` : ''}${sub ? `<span class="drawing-sub">${inline(sub)}</span>` : ''}${said}</p></div>`;
    return { html, next: at };
  }

  const NOTE_ICONS = {
    star: ['Star', 'amber', '<path d="M12 3.8l2.5 5.1 5.6.8-4.1 4 1 5.6-5-2.7-5 2.7 1-5.6-4.1-4 5.6-.8z"/>'],
    warning: ['Warning', 'orange', '<path d="M12 4L2.8 19.5h18.4z"/><path d="M12 10v4.5M12 17.2v.3"/>'],
    info: ['Info', 'blue', '<circle cx="12" cy="12" r="8.5"/><path d="M12 11v5.5M12 7.8v.3"/>'],
    check: ['Done', 'green', '<circle cx="12" cy="12" r="8.5"/><path d="M8 12.3l2.7 2.7L16 9.5"/>'],
    cross: ['No', 'red', '<circle cx="12" cy="12" r="8.5"/><path d="M9 9l6 6M15 9l-6 6"/>'],
    question: ['Question', 'violet', '<circle cx="12" cy="12" r="8.5"/><path d="M9.6 9.6a2.5 2.5 0 1 1 3.4 2.3c-.6.3-1 .8-1 1.5v.4M12 16.8v.3"/>'],
    idea: ['Idea', 'amber', '<path d="M8.2 14.2A6 6 0 1 1 15.8 14.2c-.8.7-1.3 1.6-1.3 2.6v.7h-5v-.7c0-1-.5-1.9-1.3-2.6"/><path d="M9 17.5h6M10 20.5h4"/>'],
    heart: ['Heart', 'pink', '<path d="M12 19.5s-7.5-4.4-7.5-9.7A4.3 4.3 0 0 1 12 7.2a4.3 4.3 0 0 1 7.5 2.6c0 5.3-7.5 9.7-7.5 9.7z"/>'],
    fire: ['Fire', 'orange', '<path d="M12 21c-3.9 0-6.5-2.6-6.5-6.2 0-3.4 2.6-5.3 3.6-8.3.9 1.6 1.4 2.8 1.4 4.3 1.6-1.2 2.6-3.5 2.4-6.3 3.5 2.3 5.6 6.2 5.6 10.1 0 3.7-2.6 6.4-6.5 6.4z"/>'],
    flag: ['Flag', 'red', '<path d="M5.5 4.5h11l-2 4 2 4h-11"/><path d="M5.5 21V4"/>'],
    pin: ['Pin', 'red', '<path d="M12 21s-6-5.6-6-10.5a6 6 0 0 1 12 0C18 15.4 12 21 12 21z"/><circle cx="12" cy="10.5" r="2.2"/>'],
    bookmark: ['Bookmark', 'blue', '<path d="M7 3.5h10v17l-5-3.6-5 3.6z"/>'],
    calendar: ['Date', 'blue', '<rect x="4" y="5.5" width="16" height="15" rx="2"/><path d="M4 10h16M8.5 3.5v4M15.5 3.5v4"/>'],
    clock: ['Time', 'teal', '<circle cx="12" cy="12" r="8.5"/><path d="M12 7.5V12l3 2"/>'],
    target: ['Goal', 'red', '<circle cx="12" cy="12" r="7.5"/><circle cx="12" cy="12" r="3"/><path d="M12 2.5v3M12 18.5v3M2.5 12h3M18.5 12h3"/>'],
  };

  const noteIcon = (name, hidden = false) => `<svg class="note-icon" data-tone="${NOTE_ICONS[name][1]}" viewBox="0 0 24 24" ${hidden ? 'aria-hidden="true"' : `role="img" aria-label="${NOTE_ICONS[name][0]}"`}>${NOTE_ICONS[name][2]}</svg>`;

  function inline(text) {
    const slots = [];
    const MARK = /\u0000(\d+)\u0000/g;
    const raw = (t) => t.replace(MARK, (_, i) => slots[Number(i)].raw);
    const keep = (html, source = '') => `\u0000${slots.push({ html, raw: raw(source) }) - 1}\u0000`;
    let s = text.replace(/\u0000/g, '');
    s = s.replace(/`([^`]+)`/g, (whole, code) => keep(`<code>${escape(code)}</code>`, whole));
    const shown = (tex, block, whole) => {
      const source = raw(tex).trim();
      return keep(`<span class="math${block ? ' math-block' : ''}" data-tex="${escape(source)}">${escape(source)}</span>`, whole);
    };
    s = s.replace(/\\\[([\s\S]+?)\\\]/g, (whole, tex) => shown(tex, true, whole));
    s = s.replace(/\\\(([\s\S]+?)\\\)/g, (whole, tex) => shown(tex, false, whole));
    s = s.replace(/(^|[^\\])\$\$([^$\n]+?)\$\$/g, (whole, lead, tex) => lead + shown(tex, true, whole.slice(lead.length)));
    s = s.replace(/(^|[^\\$])\$(?!\s)([^$\n]+?)(?<!\s)\$(?!\d)/g, (whole, lead, tex) => lead + shown(tex, false, whole.slice(lead.length)));
    s = s.replace(/!\[\[([^\]|\[]+)(?:\|([^\]]*))?\]\]/g, (whole, name, option) => {
      const file = raw(name).trim();
      const img = picture(file, option && /^\s*\d/.test(option) ? `${file}|${raw(option)}` : file);
      return img ? keep(img, whole) : whole;
    });
    s = s.replace(/(?<!!)\[\[([^\[\]|#]+)(?:#[^\[\]|]*)?(?:\|([^\[\]]+))?\]\]/g, (whole, title, label) => {
      const name = raw(title).trim();
      return keep(`<a class="wiki-link" href="#/search/${encodeURIComponent(name)}" data-action="open-title" data-title="${escape(name)}">${label ? escape(label.trim()) : escape(name)}</a>`, whole);
    });
    s = s.replace(/!\[([^\]]*)\]\((<[^>]+>|[^)\s]+)(?:\s+"[^"]*")?\)/g, (whole, alt, target) => {
      const img = picture(raw(target), raw(alt));
      return img ? keep(img, whole) : whole;
    });
    s = s.replace(/!?\[([^\]]*)\]\(([^)\s]+)(?:\s+"[^"]*")?\)/g, (whole, label, url) => {
      const safe = safeUrl(raw(url));
      return safe ? keep(link(safe, emphasis(escape(label || safe))), whole) : whole;
    });
    s = s.replace(/\bhttps?:\/\/[^\s<>()]+[^\s<>().,;:!?'"\]]/g, (whole) => keep(link(raw(whole), escape(raw(whole))), whole));
    s = s.replace(/:([a-z]+):/g, (whole, name, at, all) => (Object.hasOwn(NOTE_ICONS, name) && !/[\w:]/.test(all[at - 1] || '') && !/[\w:]/.test(all[at + whole.length] || '') ? keep(noteIcon(name), whole) : whole));
    s = emphasis(escape(s));
    const fill = (t) => t.replace(MARK, (_, i) => fill(slots[Number(i)].html));
    return fill(s);
  }

  const FENCE = /^\s*(```|~~~)\s*([\w+-]*)/;
  const HEADING = /^\s{0,3}(#{1,6})\s+(.*?)\s*#*\s*$/;
  const RULE = /^\s{0,3}([-*_])(\s*\1){2,}\s*$/;
  const MARKER = String.raw`[-*+]|\d+[.)]|(?<low>[a-z])\k<low>?[.)]|(?<up>[A-Z])\k<up>?(?:\)|\.(?=[^\S\n]{2}|\t))`;
  const ITEM = new RegExp(String.raw`^(\s*)(${MARKER})\s+(?<text>.*)$`);
  const TABLE_RULE = /^\s*\|?\s*:?-+:?\s*(\|\s*:?-+:?\s*)*\|?\s*$/;
  const BOX = new RegExp(String.raw`^(?:${MARKER}) {1,4}\[( |x|X)\](?: |$)`);

  const MATH_ENV = /^\\begin\{((?:equation|align|aligned|gather|gathered|multline|alignat|flalign|split|cases|matrix|pmatrix|bmatrix|vmatrix|array)\*?)\}/;

  const isBox = (line) => BOX.test(line.trimStart());

  function listable(line, after) {
    const found = line.match(ITEM);
    return found && (after || !/[a-z]/i.test(found[2]) || /^[aA][.)]/.test(found[2]) || isBox(line)) ? found : null;
  }

  const cells = (row) =>
    row
      .trim()
      .replace(/^\|/, '')
      .replace(/(?<!\\)\|$/, '')
      .split(/(?<!\\)\|/)
      .map((c) => c.trim().replace(/\\\|/g, '|'));

  const aligned = (rule) => cells(rule).map((c) => (c.startsWith(':') ? (c.endsWith(':') ? 'center' : 'left') : c.endsWith(':') ? 'right' : ''));
  const cell = (tag, text, align) => `<${tag}${align ? ` class="align-${align}"` : ''}>${inline(text)}</${tag}>`;
  const NUMBER = /^[-+\u2212]?[$€£¥]?(?:\d{1,3}(?:,\d{3})+|\d*)(?:\.\d+)?%?$/;
  const numeric = (body, n) => body.some((row) => (row[n] || '').trim()) && body.every((row) => !(row[n] || '').trim() || (/\d/.test(row[n]) && NUMBER.test(row[n].trim())));

  function letterValue(marker) {
    const run = marker.slice(0, -1).toLowerCase();
    const place = run.charCodeAt(0) - 96;
    return run.length === 2 ? place * 27 : place;
  }

  const listKind = (marker) => (/^[-*+]$/.test(marker) ? 'ul' : marker.replace(/\d+/, '1').replace(/[a-z]+/, 'a').replace(/[A-Z]+/, 'A'));

  function startsBlock(line, next) {
    return (
      FENCE.test(line) ||
      HEADING.test(line) ||
      RULE.test(line) ||
      listable(line, false) ||
      /^\s*>/.test(line) ||
      /^\s*(\$\$|\\\[\s*$|\\begin\{(?:equation|align|aligned|gather|gathered|multline|alignat|flalign|split|cases|matrix|pmatrix|bmatrix|vmatrix|array)\*?\})/.test(line) ||
      (line.includes('|') && next !== undefined && TABLE_RULE.test(next) && next.includes('-'))
    );
  }

  function list(lines, state) {
    const items = lines.map((line) => {
      const m = line.match(ITEM);
      return { indent: m[1].replace(/\t/g, '    ').length, kind: listKind(m[2]), marker: m[2], raw: line, text: m.groups.text };
    });

    function build(from, indent, depth) {
      const { kind } = items[from];
      const ordered = kind !== 'ul';
      const lettered = ordered && /[a-z]/i.test(kind);
      const paren = kind.endsWith(')') ? '-paren' : '';
      const first = lettered || !ordered ? 1 : parseInt(items[from].marker, 10);
      const style = lettered ? `list-${kind[0] === 'a' ? 'lower' : 'upper'}${paren}` : depth > 1 && ordered && (depth === state.depth && state.first !== undefined ? state.first : first) > 1 ? paren && 'list-paren' || 'list-1' : depth > 1 ? `list-${(depth - 1) % 3 + 1}` : paren && 'list-paren';
      const look = style ? ` class="${style}"` : '';
      let html = ordered ? (first > 1 ? `<ol${look} start="${first}">` : `<ol${look}>`) : `<ul${look}>`;
      let i = from;
      while (i < items.length && items[i].indent >= indent) {
        const item = items[i];
        if (item.indent === indent && item.kind !== kind) break;
        if (item.indent > indent) {
          const [inner, next] = build(i, item.indent, depth + 1);
          html = html.replace(/<\/li>$/, `${inner}</li>`);
          i = next;
          continue;
        }
        let body;
        const task = item.text.match(/^\[( |x|X)\](?:\s+(.*))?$/);
        if (task && isBox(item.raw) && state.interactive) {
          state.boxes++;
          const done = task[1] !== ' ';
          body = `<label class="task${done ? ' done' : ''}"><input type="checkbox" data-box="${state.boxes}"${done ? ' checked' : ''}><span>${inline(task[2] || '')}</span></label>`;
          html += `<li class="task-item">${body}</li>`;
        } else if (task) {
          const done = task[1] !== ' ';
          html += `<li class="task-item"><label class="task${done ? ' done' : ''}"><input type="checkbox" disabled${done ? ' checked' : ''}><span>${inline(task[2] || '')}</span></label></li>`;
        } else {
          html += `<li${lettered ? ` value="${letterValue(item.marker)}"` : ''}>${inline(item.text)}</li>`;
        }
        i++;
      }
      return [html + (ordered ? '</ol>' : '</ul>'), i];
    }

    let html = '';
    let i = 0;
    while (i < items.length) {
      const [block, next] = build(i, items[i].indent, state.depth);
      html += block;
      i = next;
    }
    return html;
  }

  function blocks(lines, state) {
    let html = '';
    let i = 0;
    while (i < lines.length) {
      const line = lines[i];

      if (!line.trim()) {
        i++;
        continue;
      }

      const math = line.trim();
      const env = math.match(MATH_ENV);
      if (env || (math.startsWith('\\[') && !math.slice(2).includes('\\]') && !math.slice(2).trim())) {
        const end = env ? `\\end{${env[1]}}` : '\\]';
        const body = [env ? math : math.slice(2)];
        let closed = body[0].includes(end) && env;
        i++;
        while (!closed && i < lines.length) {
          const next = lines[i];
          i++;
          if (next.trim().endsWith(end) || next.includes(end)) {
            body.push(env ? next : next.slice(0, next.lastIndexOf(end)));
            closed = true;
          } else body.push(next);
        }
        const tex = body.join('\n').trim();
        html += `<div class="math math-block" data-tex="${escape(tex)}">${escape(tex)}</div>`;
        continue;
      }
      if (math.startsWith('$$')) {
        let tex;
        if (math.length > 4 && math.endsWith('$$')) {
          tex = math.slice(2, -2);
          i++;
        } else {
          const body = [math.slice(2)];
          i++;
          while (i < lines.length && !lines[i].trim().endsWith('$$')) {
            body.push(lines[i]);
            i++;
          }
          if (i < lines.length) body.push(lines[i].trim().slice(0, -2));
          i++;
          tex = body.join('\n');
        }
        html += `<div class="math math-block" data-tex="${escape(tex.trim())}">${escape(tex.trim())}</div>`;
        continue;
      }
      const fence = line.match(FENCE);
      if (fence) {
        const code = [];
        i++;
        while (i < lines.length && !new RegExp('^\\s*' + fence[1]).test(lines[i])) {
          if (state.interactive && isBox(lines[i])) state.boxes++;
          code.push(lines[i]);
          i++;
        }
        i++;
        if (fence[2].toLowerCase() === 'mermaid') {
          html += `<figure class="diagram"><pre class="diagram-src"><code>${escape(code.join('\n'))}</code></pre></figure>`;
          continue;
        }
        if (fence[2].toLowerCase() === 'svg') {
          const drawn = drawing(code.join('\n'));
          if (drawn) {
            const row = state.rows ? pairedRow(drawn, code.join('\n'), lines, i) : null;
            if (row) {
              html += row.html;
              i = row.next;
              continue;
            }
            html += drawn;
            continue;
          }
        }
        const lang = fence[2] ? ` class="language-${escape(fence[2])}"` : '';
        const pre = `<pre><code${lang}>${escape(code.join('\n'))}</code></pre>`;
        html += state.tools ? `<div class="code"${fence[2] ? ` data-lang="${escape(fence[2])}"` : ''}>${pre}<button type="button" class="code-copy" aria-label="Copy code" title="Copy code"><svg viewBox="0 0 24 24" aria-hidden="true"><rect x="8.5" y="8.5" width="11" height="11" rx="2"/><path d="M15.5 8.5V6a1.5 1.5 0 0 0-1.5-1.5H6A1.5 1.5 0 0 0 4.5 6v8A1.5 1.5 0 0 0 6 15.5h2.5"/></svg></button></div>` : pre;
        continue;
      }

      const heading = line.match(HEADING);
      if (heading) {
        const level = Math.min(heading[1].length + 1, 6);
        html += `<h${level}>${inline(heading[2])}</h${level}>`;
        i++;
        continue;
      }

      if (RULE.test(line)) {
        html += '<hr>';
        i++;
        continue;
      }

      if (/^\s*>/.test(line)) {
        const quoted = [];
        while (i < lines.length && /^\s*>/.test(lines[i])) {
          quoted.push(lines[i].replace(/^\s*> ?/, ''));
          i++;
        }
        const callout = quoted[0].match(/^\s*\[!([A-Za-z-]+)\]([+-]?)\s*(.*)$/);
        if (callout) {
          const kind = callout[1].toLowerCase().replace(/[^a-z-]/g, '');
          const title = callout[3].trim() || kind.charAt(0).toUpperCase() + kind.slice(1);
          const inner = blocks(quoted.slice(1), { boxes: 0, interactive: false });
          html += callout[2]
            ? `<details class="callout callout-${kind}"${callout[2] === '+' ? ' open' : ''}><summary>${inline(title)}</summary><div class="callout-body">${inner}</div></details>`
            : `<div class="callout callout-${kind}"><div class="callout-title">${inline(title)}</div><div class="callout-body">${inner}</div></div>`;
          continue;
        }
        const inner = blocks(quoted, { boxes: 0, interactive: false });
        html += `<blockquote>${inner}</blockquote>`;
        continue;
      }

      if (line.includes('|') && i + 1 < lines.length && TABLE_RULE.test(lines[i + 1]) && lines[i + 1].includes('-')) {
        const head = cells(line);
        const aligns = aligned(lines[i + 1]);
        i += 2;
        const body = [];
        while (i < lines.length && lines[i].includes('|') && lines[i].trim()) {
          body.push(cells(lines[i]));
          i++;
        }
        if (state.tools) head.forEach((_, n) => { if (!aligns[n] && numeric(body, n)) aligns[n] = 'right'; });
        const rows = body.map((row) => `<tr>${head.map((_, n) => cell('td', row[n] || '', aligns[n])).join('')}</tr>`).join('');
        html += `<div class="table"><table><thead><tr>${head
          .map((c, n) => cell('th', c, aligns[n]))
          .join('')}</tr></thead><tbody>${rows}</tbody></table></div>`;
        continue;
      }

      if (listable(line, i === 0 && state.after)) {
        const items = [line];
        i++;
        while (i < lines.length && listable(lines[i], true)) {
          items.push(lines[i]);
          i++;
        }
        html += list(items, state);
        continue;
      }

      const para = [];
      while (i < lines.length && lines[i].trim() && (para.length === 0 || !startsBlock(lines[i], lines[i + 1]))) {
        para.push(lines[i].trim());
        i++;
      }
      html += `<p>${para.map(inline).join('<br>')}</p>`;
    }
    return html;
  }

  function render(markdown, options = {}) {
    pictureDir = options.dir || '';
    try {
      return blocks(String(markdown || '').replace(/\r\n?/g, '\n').split('\n'), {
        boxes: options.boxOffset || 0,
        interactive: options.interactive !== false,
        depth: options.depth || 1,
        after: options.after,
        first: options.first,
        rows: Boolean(options.rows),
        tools: Boolean(options.noteTools),
      });
    } finally {
      pictureDir = '';
    }
  }

  function plain(markdown) {
    return String(markdown || '')
      .replace(/```[\s\S]*?```/g, ' ')
      .replace(/^\s*(?:-{3,}|\*{3,}|_{3,})\s*$/gm, ' ')
      .replace(/^\s*\|?\s*:?-+:?\s*(\|\s*:?-+:?\s*)*\|?\s*$/gm, ' ')
      .replace(/\s*\|\s*/g, ' ')
      .replace(new RegExp(String.raw`^\s*(?:#{1,6}|>|${MARKER})\s+`, 'gm'), (head) => (/^\s*[a-z]/i.test(head) && !listable(head, false) ? head : ''))
      .replace(/\[( |x|X)\]\s+/g, '')
      .replace(/!\[\[[^\]]*\]\]/g, ' ')
      .replace(/!\[([^\]]*)\]\([^)]*\)/g, ' ')
      .replace(/\[([^\]]*)\]\([^)]*\)/g, '$1')
      .replace(/<\/?u>/g, '')
      .replace(/[*_~`]/g, '')
      .replace(/\s+/g, ' ')
      .trim();
  }

  function facts(markdown) {
    const words = plain(markdown).split(' ').filter((word) => /[\p{L}\p{N}]/u.test(word)).length;
    return words ? `${words.toLocaleString('en')} ${words === 1 ? 'word' : 'words'} · ${Math.ceil(words / 238)} min read` : '0 words';
  }

  function wordDiff(before, after, most = 120000) {
    const split = (text) => String(text || '').match(/\s*(?:[\p{L}\p{N}_'’]+|\S)|\s+/gu) || [];
    const a = split(before);
    const b = split(after);
    if (a.length * b.length > most) return null;
    const width = b.length + 1;
    const kept = new Uint16Array((a.length + 1) * width);
    for (let i = a.length - 1; i >= 0; i--) {
      for (let j = b.length - 1; j >= 0; j--) {
        kept[i * width + j] = a[i].trim() === b[j].trim() ? kept[(i + 1) * width + j + 1] + 1 : Math.max(kept[(i + 1) * width + j], kept[i * width + j + 1]);
      }
    }
    const runs = [];
    const put = (kind, del, ins) => {
      const last = runs.at(-1);
      if (last && last.kind === kind) {
        last.del += del;
        last.ins += ins;
        last.words += 1;
      } else runs.push({ kind, del, ins, words: 1 });
    };
    let i = 0;
    let j = 0;
    let hidden = false;
    const gap = (word) => /^\s*/.exec(word)[0];
    while (i < a.length || j < b.length) {
      if (i < a.length && j < b.length && a[i].trim() === b[j].trim()) {
        if (gap(a[i]) !== gap(b[j]) && /\n|\t| {2}/.test(gap(a[i]) + gap(b[j]))) hidden = true;
        put('same', a[i++], b[j++]);
      } else if (i < a.length && (j === b.length || kept[(i + 1) * width + j] >= kept[i * width + j + 1])) put('del', a[i++], '');
      else put('ins', '', b[j++]);
    }
    const parts = [];
    runs.forEach((run, k) => {
      const lone = run.kind === 'same' && run.words === 1 && run.ins.trim().length <= 3 && k > 0 && k < runs.length - 1;
      if (run.kind === 'same' && !lone) {
        parts.push({ kind: 'same', text: run.ins });
        return;
      }
      let change = parts.at(-1);
      if (!change || change.kind === 'same') parts.push((change = { kind: 'change', del: '', ins: '' }));
      change.del += run.del;
      change.ins += run.ins;
    });
    parts.forEach((part, k) => {
      const space = part.kind === 'change' && k > 0 ? /^\s*/.exec(part.ins)[0] : '';
      if (!space || (part.del && !part.del.startsWith(space))) return;
      parts[k - 1].text += space;
      part.ins = part.ins.slice(space.length);
      part.del = part.del.slice(space.length);
    });
    if (hidden) return null;
    return parts.flatMap((part) => (part.kind === 'same' ? [part] : [{ kind: 'del', text: part.del }, { kind: 'ins', text: part.ins }].filter((p) => p.text)));
  }

  const api = { render, plain, facts, escape, MARKER, MATH_ENV, listKind, listable, NOTE_ICONS, noteIcon, wordDiff };
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
  else root.leoMarkdown = api;
})(typeof window !== 'undefined' ? window : globalThis);
