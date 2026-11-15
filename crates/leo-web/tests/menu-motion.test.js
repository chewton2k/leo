const {test} = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
function fixture() {
  const source = fs.readFileSync(path.join(__dirname, '../src/web/app/core.js'), 'utf8');
  const context = {matchMedia: () => ({matches:false}), getComputedStyle: () => ({clipPath:'inset(40% 0px 0px 0px)',opacity:'0.5',transform:'matrix(0.97, 0, 0, 0.97, 0, 0)'})};
  vm.createContext(context);
  vm.runInContext(source.slice(source.indexOf('const LEAVE_MS'), source.indexOf('function rel(')), context);
  const runs = [];
  const menu = {dataset:{},style:{},hidden:true,animate() { const run = {cancel() { queueMicrotask(() => run.oncancel?.()); }}; runs.push(run); return run; }};
  return {context,menu,runs};
}
test('a menu opens from the edge nearest its trigger and closes back to it, faster', () => {
  const {context,menu,runs} = fixture();
  const box = {left:0,right:200,top:100,bottom:300,width:200,height:200};
  menu.getBoundingClientRect = () => box;
  const frames = [];
  const animate = menu.animate;
  menu.animate = (keys,options) => { frames.push({keys,options}); return animate(keys,options); };
  context.unfold(menu,{getBoundingClientRect:()=>({left:80,right:120,top:320,bottom:360})});
  assert.equal(menu.style.transformOrigin,'100px 200px');
  assert.equal(frames[0].keys[0].clipPath,'inset(100% 0 0 0)');
  assert.equal(frames[0].keys[0].transform,'scale(0.95)');
  assert.equal(menu.style.pointerEvents,undefined);
  runs[0].onfinish();
  context.fold(menu);
  assert.equal(frames[1].keys[1].clipPath,'inset(100% 0 0 0)');
  assert.equal(frames[1].options.duration,144);
  context.unfold(menu,{x:50,y:150});
  assert.equal(menu.style.transformOrigin,'50px 50px');
});
test('rubber resists more the further it is pulled and never reaches its reach', () => {
  const {context} = fixture();
  const a = context.rubber(10), b = context.rubber(100), c = context.rubber(10000);
  assert.ok(a > 0 && a < 10 && b > a && c > b && c < 24);
  assert.equal(context.rubber(-10), -a);
  const pastEdge = vm.runInContext('pastEdge', context);
  assert.equal(pastEdge(50,0,100),50);
  assert.ok(pastEdge(130,0,100) > 100 && pastEdge(130,0,100) < 130);
});
test('reopening a closing menu clears its closing state immediately', () => {
  const {context,menu} = fixture();
  context.unfold(menu);
  context.fold(menu);
  assert.equal(menu.dataset.folding,'1');
  context.unfold(menu);
  assert.equal(menu.hidden,false);
  assert.equal(menu.dataset.folding,undefined);
});
test('cancelled menu animations cannot clear the state of a newer close', async () => {
  const {context,menu,runs} = fixture();
  context.unfold(menu);
  context.fold(menu);
  context.unfold(menu);
  context.fold(menu);
  await Promise.resolve();
  assert.equal(menu.dataset.folding,'1');
  runs.at(-1).onfinish();
  assert.equal(menu.hidden,true);
  assert.equal(menu.dataset.folding,undefined);
});
test('reversing a menu mid-run starts from where it is, not from a snap', () => {
  const {context,menu} = fixture();
  const frames = [];
  menu.animate = (keys) => { frames.push(keys); return {cancel() {}}; };
  context.unfold(menu);
  context.fold(menu);
  assert.equal(frames[1][0].clipPath,'inset(40% 0px 0px 0px)');
  assert.equal(frames[1][0].opacity,'0.5');
  context.unfold(menu);
  assert.equal(frames[2][0].clipPath,'inset(40% 0px 0px 0px)');
  assert.equal(frames[2][0].transform,'matrix(0.97, 0, 0, 0.97, 0, 0)');
});
