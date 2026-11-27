const test = require('node:test');
const assert = require('node:assert/strict');
const vm = require('node:vm');
const { paneHelpers } = require('./page-source');

function harness() {
  const main = { note: { id: 'main' } };
  const context = { app: { id: 'app' }, $: (selector, root) => ({ selector, root }) };
  vm.createContext(context);
  vm.runInContext(`let state = { view: 'note', session: null, main: null }; let marginPage = { s: null };\n${paneHelpers()}\npanes[0].margin = marginPage;\nconst side = { el: { id: 'beside' }, s: { note: { id: 'side' } }, margin: { s: null } };\npanes.push(side);\nglobalThis.h = { panes, here, paneOf, paneWith, inPane, withPane, marginLater, get state() { return state; }, set state(v) { state = v; }, get marginPage() { return marginPage; }, side };`, context);
  context.h.state.main = main;
  context.h.state.session = main;
  return { h: context.h, main };
}

test('withPane puts back the session and margin it swapped, even when the work throws', () => {
  const { h, main } = harness();
  const before = h.marginPage;
  assert.throws(() => h.withPane(h.side, () => {
    assert.equal(h.state.session, h.side.s);
    assert.equal(h.marginPage, h.side.margin);
    throw new Error('boom');
  }), /boom/);
  assert.equal(h.state.session, main);
  assert.equal(h.marginPage, before);
});

test('withPane restores the page it swapped, not a page drawn meanwhile', () => {
  const { h, main } = harness();
  const was = h.state;
  h.withPane(h.side, () => { h.state = { view: 'workspace' }; });
  assert.equal(was.session, main);
  assert.equal(h.state.session, undefined);
});

test('paneWith and paneOf find the pane holding a note or session', () => {
  const { h, main } = harness();
  assert.equal(h.paneWith('main'), h.panes[0]);
  assert.equal(h.paneWith('side'), h.side);
  assert.equal(h.paneWith('gone'), undefined);
  assert.equal(h.paneOf(main), h.panes[0]);
  assert.equal(h.paneOf(h.side.s), h.side);
  assert.equal(h.paneOf(null), undefined);
});

test('here and inPane default to the focused pane, and the main pane without a note', () => {
  const { h } = harness();
  assert.equal(h.here(), h.panes[0]);
  assert.equal(h.inPane('#doc').root.id, 'app');
  h.withPane(h.side, () => {
    assert.equal(h.here(), h.side);
    assert.equal(h.inPane('#doc').root.id, 'beside');
    assert.equal(h.inPane('#doc', h.panes[0]).root.id, 'app');
  });
  h.state.session = null;
  assert.equal(h.here(), h.panes[0]);
});

test('marginLater runs its work in the pane that was focused when it was scheduled', () => {
  const { h, main } = harness();
  const later = h.withPane(h.side, () => h.marginLater((n) => [h.state.session, h.marginPage, n]));
  const [session, margin, n] = later(7);
  assert.equal(session, h.side.s);
  assert.equal(margin, h.side.margin);
  assert.equal(n, 7);
  assert.equal(h.state.session, main);
});
