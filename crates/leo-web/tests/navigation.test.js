const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { pageHtml } = require('./page-source');

const read = (file) => fs.readFileSync(path.join(__dirname, '../src', file), 'utf8').replace(/\r\n/g, '\n');

test('graph page has no navigation, actions, route or bundled renderer', () => {
  for (const file of ['web/app/side.js', 'web/app/sheets.js', 'web/app/actions.js', 'web/app/events.js', 'web/app/core.js']) {
    assert.doesNotMatch(read(file), /sideRow\('map'|#\/map|showMap|leaveMap|['"]map-[a-z-]+['"]|note-map/, file);
  }
  assert.doesNotMatch(read('routes/assets.rs'), /web\/graph\.js|app\/map\.js/);
  assert.doesNotMatch(read('routes/mod.rs'), /\.route\("\/(?:api\/graph|graph\.js)/);
  const page = pageHtml();
  assert.match(page, /\.side-rail\b[\s\S]*<\/html>/);
  assert.doesNotMatch(page, /src="\/graph\.js"|on-map|\.map-(?:tools|panel|zoom)/);
});

test('shared count wording works without the removed graph script', () => {
  const vm = require('node:vm');
  const core = read('web/app/core.js');
  const declaration = core.match(/^const plural = .*;$/m);
  assert.ok(declaration, 'count wording belongs to the shared app code');
  const count = vm.runInNewContext(`${declaration[0]} plural;`);
  assert.equal(count(0, 'note'), '0 notes');
  assert.equal(count(1, 'note'), '1 note');
  assert.equal(count(2, 'note'), '2 notes');
  assert.equal(count(2, 'class', 'classes'), '2 classes');
});

test('creation chooses explicit target, selected folder, open-note folder, then root', () => {
  const vm = require('node:vm');
  const declaration = read('web/app/side.js').match(/^const creationDir = [\s\S]*?\n};$/m)[0];
  const context = { side: { target: null, root: '' }, state: { view: 'workspace', dir: '' } };
  vm.createContext(context);
  const choose = vm.runInContext(`${declaration} creationDir;`, context);
  assert.equal(choose(), '');
  context.state = { view: 'note', dir: 'Open' };
  assert.equal(choose(), 'Open');
  context.side.target = 'Selected';
  assert.equal(choose(), 'Selected');
  assert.equal(choose('Explicit'), 'Explicit');
  assert.equal(choose(''), '');
});

test('context targets capture ids and folders without changing the current note', () => {
  const vm = require('node:vm');
  const side = read('web/app/side.js');
  const source = side.slice(side.indexOf('function explorerTarget('), side.indexOf('\nfunction explorerMenu('));
  const target = vm.runInNewContext(`${source} explorerTarget;`, { creationDir: () => 'Scope', side: { root: '' } });
  const plain = (value) => JSON.parse(JSON.stringify(value));
  const el = ({ note, folder, group, region } = {}) => ({ closest: selector => selector === '.side-note-wrap' && note ? { querySelector: () => ({ dataset: note }) } : selector === '.side-folder' && folder ? { dataset: { dropDir: folder } } : selector === '.side-group' && group ? {} : selector === '.side-note-status, .side-kids' && region !== undefined ? { dataset: { dropDir: region } } : null });
  assert.deepEqual(plain(target(el({ note: { id: 'note-a', from: 'Folder A' } }))), { kind: 'note', id: 'note-a', dir: 'Folder A' });
  assert.deepEqual(plain(target(el({ folder: 'Folder B' }))), { kind: 'folder', dir: 'Folder B' });
  assert.deepEqual(plain(target(el())), { kind: 'root', dir: '' });
  assert.deepEqual(plain(target(el({ group: true }))), { kind: 'root', dir: 'Scope' });
  assert.deepEqual(plain(target(el({ region: 'Folder C/Child' }))), { kind: 'root', dir: 'Folder C/Child' });
  assert.deepEqual(plain(target(el({ region: '' }))), { kind: 'root', dir: '' });
});

test('move choices leave out where the note or folder already is', () => {
  const vm = require('node:vm');
  const side = read('web/app/side.js');
  const source = side.slice(side.indexOf('async function explorerMoveMenu('), side.indexOf('\nasync function afterExplorerNote('));
  const shown = [];
  const context = {
    seq: 1,
    api: async () => ['A', 'A/B', 'A/B/C', 'D'].map((name) => ({ name })),
    parentOf: (path) => path.split('/').slice(0, -1).join('/'),
    inFolder: (dir, folder) => dir === folder || dir.startsWith(`${folder}/`),
    esc: (text) => text,
    folderLabel: (dir) => dir.split('/').pop() || 'Top level',
    ICON: { folder: '' },
    popupMenu: (owner, { rows }) => { shown.push(rows.map((row) => (row.text === 'Top level' ? '' : row.text))); },
  };
  const move = vm.runInNewContext(`${source} explorerMoveMenu;`, context);
  return (async () => {
    await move({ kind: 'note' }, { title: 'N', directory: 'A/B' });
    await move({ kind: 'note' }, { title: 'N', directory: '' });
    await move({ kind: 'folder', dir: 'A/B' }, null);
    await move({ kind: 'folder', dir: 'D' }, null);
    assert.deepEqual(JSON.parse(JSON.stringify(shown)), [['', 'A', 'A/B/C', 'D'], ['A', 'A/B', 'A/B/C', 'D'], ['', 'D'], ['A', 'A/B', 'A/B/C']]);
  })();
});

test('the page is assembled from every stylesheet in order, between the head and tail', () => {
  const assets = read('routes/assets.rs');
  const listed = [...assets.matchAll(/include_str!\("\.\.\/web\/(styles\/[^"]+)"\)/g)].map((m) => m[1]);
  const onDisk = fs.readdirSync(path.join(__dirname, '../src/web/styles')).filter((name) => name.endsWith('.css')).sort().map((name) => `styles/${name}`);
  assert.ok(listed.length > 10);
  assert.deepEqual(listed, onDisk);
  const parts = [...assets.matchAll(/include_str!\("\.\.\/web\/([^"]+)"\)/g)].map((m) => m[1]);
  const page = parts.slice(0, parts.indexOf('page-tail.html') + 1);
  assert.deepEqual(page, [onDisk[0], 'dark-tokens.css', 'dark-tokens.css', 'page-head.html', ...onDisk.slice(1), 'page-tail.html']);
  assert.match(assets, /include_str!\("\.\.\/web\/page-head\.html"\),\s*theme!\(\),\s*include_str!\("\.\.\/web\/styles\/02-base\.css"\)/);
});

test('the app container is not a live region, and a separate status line announces the page', () => {
  const html = pageHtml();
  assert.doesNotMatch(html, /<main id="app"[^>]*aria-live/);
  assert.match(html, /<div id="route-say" class="fa-sr" role="status"><\/div>/);
  assert.match(read('web/app/events.js'), /\$\('#route-say'\)\.textContent = document\.title/);
});
