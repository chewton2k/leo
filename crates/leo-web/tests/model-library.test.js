const { test } = require('node:test');
const assert = require('node:assert/strict');
const vm = require('node:vm');
const fs = require('node:fs');
const path = require('node:path');
function helpers() {
  const source = fs.readFileSync(path.join(__dirname, '../src/web/app/settings.js'), 'utf8');
  const context = { state: { speechFilter: 'recommended' }, esc: require('../src/web/markdown.js').escape, ICON: { chevron: '' }, settingsOption: () => '', knobStretch() {}, plural: (n, word, many = `${word}s`) => `${n} ${n === 1 ? word : many}`, app: { addEventListener() {} } };
  vm.createContext(context);
  const nav = fs.readFileSync(path.join(__dirname, '../src/web/app/settings-navigation.js'), 'utf8');
  vm.runInContext(nav.slice(nav.indexOf('function segmented('), nav.indexOf('function pickSegment(')), context);
  vm.runInContext(source.slice(source.indexOf('function speechCards(')), context);
  return context;
}
test('Downloaded lists every disk model even when filtered or damaged', () => {
  const h = helpers();
  const models = [{ id: 'one', name: 'English', languages: ['en'], size: 100, bytes: 100, downloaded: true, verified: true, in_use: true, recommended: true, speed: 8, accuracy: 96 }, { id: 'two', name: 'Damaged', languages: ['ja'], size: 100, bytes: 20, downloaded: true, verified: false, recommended: false, speed: 0, accuracy: 0 }];
  const html = h.speechCards({ selected: { id: 'one', language: 'en' }, ready: true, disk_bytes: 120, models, extra_files: [{ name: 'older.gguf', bytes: 10 }] });
  assert.ok(html.includes('<h3>Downloaded</h3>'));
  assert.ok(html.includes('Damaged'));
  assert.ok(html.includes('older.gguf'));
  assert.ok(html.includes('Check failed'));
  assert.ok(html.includes('Multilingual'));
  assert.ok(html.includes('Accuracy'));
  assert.ok(html.includes('Speed'));
  assert.equal((html.match(/data-model-row="one"/g) || []).length, 1);
  assert.ok(!html.includes('data-filter="downloaded"'));
  assert.equal(h.speechFiltered(models, 'english').length, 1);
  assert.equal(h.speechFiltered(models, 'many').length, 0);
});
test('model rows show the catalog description capabilities size accuracy and speed', () => {
  const h = helpers();
  const html = h.speechModelRow({id:'parakeet',name:'Parakeet Unified EN 0.6B',description:'Fast, accurate <live> English transcription',languages:['en'],size:731357568,downloaded:true,verified:true,in_use:true,recommended:true,streaming:true,live:true,speed:13.955,accuracy:96.01,accuracy_measured:true,benchmark_language:'en'});
  for (const text of ['Fast, accurate &lt;live&gt; English transcription','English only','Streaming','697 MB','Accuracy','96.0%','Speed','14.0× real time','Recommended','In use','Delete']) assert.ok(html.includes(text),text);
  assert.ok(html.includes('<meter'));
  assert.ok(!html.includes('<live>'));
  const unknown = h.speechModelRow({id:'unknown',name:'Unknown',languages:['ja'],size:1,speed:0,accuracy:0,accuracy_measured:false});
  assert.equal((unknown.match(/Not measured/g)||[]).length,2);
});
test('the downloaded heading counts models in the singular', () => {
  const h = helpers();
  const models = [{ id: 'one', name: 'English', languages: ['en'], size: 100, bytes: 100, downloaded: true, verified: true, recommended: true, speed: 8, accuracy: 96 }];
  assert.ok(h.speechCards({ selected: { id: 'one', language: 'en' }, ready: true, disk_bytes: 100, models, extra_files: [] }).includes('1 model ·'));
});
test('a model that cannot detect language is used with the system language when it has it, else English, else its first', () => {
  const h = helpers();
  const model = { languages: ['bg', 'de', 'en'], detects_language: false };
  const use = (language) => { h.navigator = { language }; return h.speechUseLanguage(model); };
  assert.equal(use('de-CH'), 'de');
  assert.equal(use('EN-us'), 'en');
  assert.equal(use('ja-JP'), 'en');
  assert.equal(use(undefined), 'en');
  h.navigator = { language: 'ja-JP' };
  assert.equal(h.speechUseLanguage({ languages: ['bg', 'de'], detects_language: false }), 'bg');
  assert.equal(h.speechUseLanguage({ languages: ['en', 'de'], detects_language: true }), 'auto');
  assert.equal(h.speechUseLanguage({ languages: ['en', 'de'] }), 'auto');
});
test('Detect language is offered only for models that detect language', () => {
  const h = helpers();
  h.settingsOption = (value) => `<option value="${value}">`;
  const html = (extra) => h.speechCards({ selected: { id: 'm', language: 'en' }, ready: true, disk_bytes: 1, extra_files: [], models: [{ id: 'm', name: 'M', languages: ['en', 'de'], size: 1, bytes: 1, downloaded: true, verified: true, in_use: true, speed: 8, accuracy: 90, ...extra }] });
  assert.ok(html({ detects_language: true }).includes('<option value="auto">'));
  assert.ok(html({}).includes('<option value="auto">'));
  assert.ok(!html({ detects_language: false }).includes('<option value="auto">'));
  assert.ok(html({ detects_language: false }).includes('<option value="de">'));
});
test('sizes under a megabyte are written in KB', () => {
  const h = helpers();
  assert.equal(h.speechBytes(921600), '900 KB');
  assert.equal(h.speechBytes(1048576), '1 MB');
  assert.equal(h.speechBytes(2400000), '2 MB');
  assert.equal(h.speechBytes(1610612736), '1.5 GB');
  assert.equal(h.speechBytes(512), '512 B');
});
