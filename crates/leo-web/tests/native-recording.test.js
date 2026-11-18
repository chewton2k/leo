const { test } = require('node:test');
const assert = require('node:assert/strict');
require('../src/web/recording.js');

test('every new app source uses Rust capture without browser audio support', () => {
  for (const local of [true, false]) for (const canShare of [true, false]) {
    assert.equal(global.leoRecording.sourceFor('microphone', { local, canShare }), 'microphone');
    assert.equal(global.leoRecording.sourceFor('screen', { local, canShare }), 'screen');
  }
});

test('pasting a screenshot accepts only one supported picture and keeps text paste alone', () => {
  const image = { type: 'image/png', size: 100 };
  const file = { kind: 'file', type: image.type, getAsFile: () => image };
  assert.equal(global.leoRecording.pastedScreenshot({ items: [file] }), image);
  assert.equal(global.leoRecording.pastedScreenshot({ items: [{ kind: 'string', type: 'text/plain' }] }), null);
  assert.equal(global.leoRecording.pastedScreenshot({ items: [{ ...file, type: 'image/svg+xml' }] }), null);
});

test('late screenshot encoding cannot attach it to another recording', async () => {
  let current = { id: 'first', state: 'recording', secs: 42 };
  let finish;
  const encoding = new Promise((resolve) => { finish = resolve; });
  let calls = 0;
  const saved = global.leoRecording.saveScreenshot(current, { type: 'image/png', size: 5 }, () => encoding, () => { calls++; }, () => current);
  current = { id: 'second', state: 'recording', secs: 1 };
  finish('image');
  assert.equal(await saved, null);
  assert.equal(calls, 0);
});

test('a screenshot of any size is sent, and only a file that is not a picture is refused', async () => {
  const current = { id: 'first', state: 'recording', secs: 42 };
  const sent = [];
  const big = { type: 'image/png', size: 300 * 1024 * 1024 };
  const saved = await global.leoRecording.saveScreenshot(current, big, async () => 'data', async (url, options) => { sent.push([url, options.body]); return { ok: true }; }, () => current);
  assert.deepEqual(saved, { ok: true });
  assert.deepEqual(sent, [['/api/record/first/snapshot', { data: 'data', at: 42 }]]);
  await assert.rejects(global.leoRecording.saveScreenshot(current, { type: 'image/svg+xml', size: 5 }, async () => 'data', async () => ({}), () => current), /Paste a PNG, JPEG, GIF or WebP screenshot\./);
});
