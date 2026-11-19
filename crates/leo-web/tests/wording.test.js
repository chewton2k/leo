const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

const web = path.join(__dirname, '../src/web');
const apart = new Set(['margin.js', path.join('app', 'margin-notes.js')]);

function scripts(dir, base = '') {
  return fs.readdirSync(path.join(dir, base), { withFileTypes: true }).flatMap((entry) => {
    const rel = path.join(base, entry.name);
    if (entry.isDirectory()) return entry.name === 'vendor' ? [] : scripts(dir, rel);
    return entry.name.endsWith('.js') && !apart.has(rel) ? [rel] : [];
  });
}

test('the page words Felix being busy, a taken folder name and the product name one way', () => {
  const files = scripts(web);
  assert.ok(files.includes('chat.js') && files.includes(path.join('app', 'sheets.js')));
  const old = [/finishing another answer/, /That folder already exists/, /['"`>] ?Leo (could|did)/];
  for (const file of files) {
    const text = fs.readFileSync(path.join(web, file), 'utf8');
    for (const pattern of old) assert.doesNotMatch(text, pattern, `${file} has ${pattern}`);
  }
});
