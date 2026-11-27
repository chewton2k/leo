const fs = require('node:fs');
const path = require('node:path');

const web = path.resolve(__dirname, '../src/web');
const read = (name) => fs.readFileSync(path.join(web, name), 'utf8');

function darkBlocks() {
  const dark = read('dark-tokens.css');
  return `@media (prefers-color-scheme: dark) {\n  :root:not([data-theme="light"]) {\n${dark}  }\n}\n:root[data-theme="dark"] {\n${dark}  color-scheme: dark;\n}\n:root[data-theme="light"] { color-scheme: light; }\n`;
}

function pageCss() {
  const dir = path.join(web, 'styles');
  return fs
    .readdirSync(dir)
    .filter((name) => name.endsWith('.css'))
    .sort()
    .map((name) => fs.readFileSync(path.join(dir, name), 'utf8') + (name === '01-tokens.css' ? darkBlocks() : ''))
    .join('');
}

function paneHelpers() {
  const core = read('app/core.js');
  return core.slice(core.indexOf('const panes ='), core.indexOf('const SEARCH_HIGHLIGHT_MS'));
}

function pageHtml() {
  return read('page-head.html') + pageCss() + read('page-tail.html');
}

module.exports = { pageCss, pageHtml, paneHelpers };
