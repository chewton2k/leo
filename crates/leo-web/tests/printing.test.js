const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

test('native commands have explicit permissions scoped to their own window and origin', () => {
  const desktop = path.resolve(__dirname, '../../leo-desktop');
  const read = file => JSON.parse(fs.readFileSync(file, 'utf8'));
  const files = folder => fs.existsSync(folder) ? fs.readdirSync(folder).filter(name => name.endsWith('.json')).map(name => read(path.join(folder, name))) : [];
  const permissions = files(path.join(desktop, 'permissions')).flatMap(file => file.permission || []);
  const capabilities = files(path.join(desktop, 'capabilities'));
  for (const [command, window, remote] of [['print_page', 'main', true], ['dictation_popup', 'dictation', false]]) {
    const grants = permissions.filter(permission => permission.commands?.allow?.includes(command));
    assert.equal(grants.length, 1, `${command} needs an explicit permission`);
    assert.deepEqual(grants[0].commands.allow, [command]);
    const callers = capabilities.filter(capability => capability.permissions.includes(grants[0].identifier));
    assert.equal(callers.length, 1, `${command} needs one scoped capability`);
    assert.deepEqual(callers[0].windows, [window]);
    assert.deepEqual(callers[0].webviews || [], []);
    assert.equal(callers[0].local !== false, !remote);
    assert.deepEqual(callers[0].remote?.urls || [], remote ? ['http://127.0.0.1:*', 'http://localhost:*'] : []);
  }
});

test('desktop printing keeps the Windows native print function and bridges only macOS', async () => {
  const desktop = path.resolve(__dirname, '../../leo-desktop/src');
  const rust = fs.readFileSync(path.join(desktop, 'lib.rs'), 'utf8');
  const script = rust.match(/const PRINT: &str = "(.*)";/)?.[1] || fs.readFileSync(path.join(desktop, 'print.js'), 'utf8');
  for (const platform of ['mac', 'windows']) {
    let native = 0, bridge = 0;
    const original = () => { native++; };
    const window = { __LEO_APP__: platform, print: original, __TAURI_INTERNALS__: { invoke: async command => { assert.equal(command, 'print_page'); bridge++; } } };
    vm.runInNewContext(script, { window, Promise, setTimeout });
    const printed = window.print();
    assert.equal(bridge, 0, 'the note title reaches the page before the native print job reads it');
    await printed;
    assert.equal(native, platform === 'windows' ? 1 : 0);
    assert.equal(bridge, platform === 'mac' ? 1 : 0);
  }
});

test('PDF export reports print errors and restores its title', async () => {
  const source = fs.readFileSync(path.resolve(__dirname, '../src/web/app/sheets.js'), 'utf8');
  const share = source.slice(source.indexOf('async function share('), source.indexOf('async function openTitle'));
  for (const rejects of [false, true]) {
    const listeners = new Map(), errors = [];
    const context = { seq: 1, document: { title: 'Leo' }, ready: async () => ({ note: { title: 'My note' } }), toast: (text, options) => errors.push([text, options]), window: { addEventListener: (name, fn) => listeners.set(name, fn), removeEventListener: name => listeners.delete(name), print: async () => { if (rejects) throw Error('raw programmer error'); listeners.get('afterprint')(); } } };
    vm.createContext(context);
    vm.runInContext(share, context);
    await context.share();
    assert.equal(context.document.title, 'Leo');
    assert.equal(listeners.size, 0);
    assert.equal(errors.length, rejects ? 1 : 0);
    if (rejects) assert.match(errors[0][0], /print|PDF/i);
  }
});

test('finishing a save or print never prints or renames a newer page', async () => {
  const source = fs.readFileSync(path.resolve(__dirname, '../src/web/app/sheets.js'), 'utf8');
  const share = source.slice(source.indexOf('async function share('), source.indexOf('async function openTitle'));
  for (const navigate of ['save', 'print']) {
    const listeners = new Map();
    let printed = 0;
    const value = { seq: 1, document: { title: 'Leo' }, toast() {}, window: { addEventListener: (name, fn) => listeners.set(name, fn), removeEventListener: name => listeners.delete(name), print: async () => { printed++; value.seq++; value.document.title = 'New page'; listeners.get('afterprint')(); } } };
    value.ready = async () => { if (navigate === 'save') { value.seq++; value.document.title = 'New page'; } return { note: { title: 'My note' } }; };
    vm.createContext(value);
    vm.runInContext(share, value);
    await value.share();
    assert.equal(printed, navigate === 'save' ? 0 : 1);
    assert.equal(value.document.title, 'New page');
    assert.equal(listeners.size, 0);
  }
  let message;
  const empty = { seq: 1, ready: async () => null, toast: text => { message = text; } };
  vm.createContext(empty);
  vm.runInContext(share, empty);
  await empty.share();
  assert.match(message, /Write something first/);
});

function shareHarness(extra = {}) {
  const source = fs.readFileSync(path.resolve(__dirname, '../src/web/app/sheets.js'), 'utf8');
  const code = source.slice(source.indexOf('async function noteMarkdown()'), source.indexOf('async function openTitle'));
  const log = { toasts: [], menus: [], clicks: [], printed: 0 };
  const context = {
    seq: 1, ICON: { note: 'N', download: 'D', share: 'S' }, marginPrintable: () => [], document: { title: 'Leo', createElement: () => ({ click() { log.clicks.push(this); } }) },
    ready: async () => ({ note: { title: 'Saved title' }, id: 1 }), snapshot: () => ({ title: 'Cell biology', body: 'First line\n- [ ] one\n' }),
    toast: (text, options) => log.toasts.push([text, options]), popupMenu: (owner, menu) => log.menus.push([owner, menu]),
    navigator: { clipboard: { writeText: async (text) => { log.copied = text; } } },
    URL: { createObjectURL: (blob) => { log.blob = blob; return 'blob:x'; }, revokeObjectURL() {} }, Blob: class { constructor(parts, options) { this.parts = parts; this.options = options; } },
    window: { addEventListener() {}, removeEventListener() {}, print: async () => { log.printed++; } }, setTimeout: () => 0, ...extra,
  };
  vm.createContext(context);
  vm.runInContext(code, context);
  return { context, log };
}

test('Share opens a menu of Copy as Markdown, Save .md file and Export as PDF', () => {
  const { context, log } = shareHarness();
  const owner = {};
  context.shareMenu(owner);
  const [menuOwner, menu] = log.menus[0];
  assert.equal(menuOwner, owner);
  assert.equal(menu.label, 'Share');
  assert.deepEqual(JSON.parse(JSON.stringify(menu.rows.map((row) => [row.text, row.icon]))), [['Copy as Markdown', 'N'], ['Save .md file', 'D'], ['Export as PDF', 'S']]);
  assert.ok(menu.rows.every((row) => typeof row.run === 'function'));
});

test('Copy as Markdown writes the title and body, and says why when it cannot', async () => {
  const { context, log } = shareHarness();
  await context.shareMenu({});
  await log.menus[0][1].rows[0].run();
  assert.equal(log.copied, '# Cell biology\n\nFirst line\n- [ ] one\n');
  assert.equal(log.toasts.length, 1);
  assert.equal(log.toasts[0][0], 'Copied “Cell biology” as Markdown');
  const refused = shareHarness({ navigator: { clipboard: { writeText: async () => { throw Error('Permission denied'); } } } });
  refused.context.shareMenu({});
  await refused.log.menus[0][1].rows[0].run();
  assert.equal(refused.log.toasts[0][0], 'Could not copy: Permission denied');
  assert.equal(refused.log.toasts[0][1].bad, true);
});

test('Save .md file downloads the same Markdown under the note title', async () => {
  const { context, log } = shareHarness({ snapshot: () => ({ title: 'a/b: c', body: 'Body' }) });
  context.shareMenu({});
  await log.menus[0][1].rows[1].run();
  assert.equal(log.blob.parts[0], '# a/b: c\n\nBody\n');
  assert.equal(log.clicks[0].download, 'a-b- c.md');
  assert.equal(log.clicks[0].href, 'blob:x');
});

test('Export as PDF from the menu still prints, and an unsaved note says to write first', async () => {
  const { context, log } = shareHarness();
  context.shareMenu({});
  await log.menus[0][1].rows[2].run();
  assert.equal(log.printed, 1);
  const empty = shareHarness({ ready: async () => null });
  empty.context.shareMenu({});
  await empty.log.menus[0][1].rows[0].run();
  await empty.log.menus[0][1].rows[1].run();
  assert.equal(empty.log.copied, undefined);
  assert.equal(empty.log.clicks.length, 0);
  assert.deepEqual(empty.log.toasts.map((t) => t[0]), ['Write something first.', 'Write something first.']);
});

function notesHarness({ notes = [{ at: { from: 0, to: 9 }, text: 'Rows sum to one.' }, { at: { from: 20, to: 30 }, text: 'Name the damping factor.' }], fails = false, title = 'Markov "chains"' } = {}) {
  const live = [], listeners = new Map(), log = { printed: [], toasts: [], normalized: 0, menus: [] };
  const place = (node) => { live.push(node); node.parentNode = { normalize() { log.normalized++; } }; };
  const element = (tag) => ({ tag, dataset: {}, children: [], hidden: false, textContent: '', get childElementCount() { return this.children.length; }, append(...kids) { this.children.push(...kids); }, after(node) { place(node); }, remove() { live.splice(live.indexOf(this), 1); } });
  const s = { note: { title }, doc: { domPoint: (at) => ({ node: { nodeType: 3 }, offset: at }), blockAt: () => null } };
  const context = {
    seq: 1, ICON: { note: 'N', download: 'D', share: 'S' }, Node: { TEXT_NODE: 3 }, marginPage: { s },
    marginPrintable: () => notes, marginAnswer: (item, text) => { item.textContent = text; },
    $: () => element('div'), ready: async () => s, snapshot: () => ({ title: 'x', body: '' }), toast: (text) => log.toasts.push(text), popupMenu: (owner, menu) => log.menus.push(menu),
    document: { title: 'Leo', head: { append: place }, createElement: element, createRange: () => ({ setStart() {}, selectNodeContents() {}, collapse() {}, insertNode: place }), querySelectorAll: (q) => (q === '[data-print]' ? [...live] : []) },
    window: {
      addEventListener: (name, fn) => listeners.set(`${name}:${fn.name}`, fn), removeEventListener: (name, fn) => listeners.delete(`${name}:${fn.name}`),
      print: async () => { log.printed.push(live.map((n) => n.tag)); if (fails) throw Error('no dialog'); },
    },
  };
  vm.createContext(context);
  const source = fs.readFileSync(path.resolve(__dirname, '../src/web/app/sheets.js'), 'utf8');
  vm.runInContext(source.slice(source.indexOf('async function noteMarkdown()'), source.indexOf('async function openTitle')), context);
  return { context, live, listeners, log, fire: (name) => [...listeners].filter(([key]) => key.startsWith(`${name}:`)).forEach(([, fn]) => fn()) };
}

test('Export as PDF with Felix’s notes shows only when the note has open margin notes, and prints them numbered after the text', async () => {
  const none = notesHarness({ notes: [] });
  none.context.shareMenu({});
  assert.deepEqual([...none.log.menus[0].rows].map((row) => row.text), ['Copy as Markdown', 'Save .md file', 'Export as PDF']);
  const { context, live, log } = notesHarness();
  context.shareMenu({});
  const rows = [...log.menus[0].rows];
  assert.deepEqual(rows.map((row) => row.text), ['Copy as Markdown', 'Save .md file', 'Export as PDF', 'Export as PDF with Felix’s notes']);
  await rows[3].run();
  assert.deepEqual(log.printed[0], ['sup', 'sup', 'section', 'style']);
  assert.deepEqual(live.filter((n) => n.tag === 'sup').map((n) => [n.dataset.print, n.hidden, n.children.length, n.textContent]), [['1', true, 0, ''], ['2', true, 0, '']]);
  assert.deepEqual(live.find((n) => n.tag === 'section').children[0].children.map((li) => li.textContent), ['Rows sum to one.', 'Name the damping factor.']);
  assert.match(live.find((n) => n.tag === 'style').textContent, /content: "Markov \\"chains\\" · " counter\(page\) " \/ " counter\(pages\)/);  const untitled = notesHarness({ title: '' });
  await untitled.context.share(true);
  assert.deepEqual(untitled.log.printed[0], ['sup', 'sup', 'section'], 'an untitled note keeps the plain page counter');
});

test('notes printed without an afterprint never reach a later plain export, and clear on the next press, afterprint or a print error', async () => {
  const quiet = notesHarness();
  await quiet.context.share(true);
  assert.equal(quiet.live.length, 4);
  assert.ok(quiet.listeners.has('pointerdown:printNotesClear') && quiet.listeners.has('keydown:printNotesClear'));
  await quiet.context.share();
  assert.deepEqual(quiet.log.printed[1], []);
  assert.equal(quiet.live.length, 0);
  assert.equal(quiet.log.normalized, 4);
  assert.ok(![...quiet.listeners.keys()].some((key) => key.endsWith(':printNotesClear')));
  for (const event of ['pointerdown', 'keydown', 'afterprint']) {
    const h = notesHarness();
    await h.context.share(true);
    h.fire(event);
    assert.equal(h.live.length, 0, event);
  }
  const failed = notesHarness({ fails: true });
  await failed.context.share(true);
  assert.equal(failed.live.length, 0);
  assert.equal(failed.log.toasts.length, 1);  const broken = notesHarness();
  let points = 0;
  broken.context.marginPage.s.doc.domPoint = (at) => {
    if (++points > 1) throw Error('lost the passage');
    return { node: { nodeType: 3 }, offset: at };
  };
  await broken.context.share(true);
  assert.equal(broken.live.length, 0, 'marks placed before a failure are cleared');
  assert.deepEqual(broken.log.printed, [], 'nothing prints half-marked');
  assert.equal(broken.log.toasts.length, 1);
});
