const assert = require('node:assert/strict');
const { test } = require('node:test');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const file = path.resolve(__dirname, '../src/web/pane-resize.js');

class Handle extends EventTarget {
  constructor() { super(); this.attributes = new Map(); this.captured = new Set(); }
  setAttribute(key,value) { this.attributes.set(key,value); }
  focus() {}
  setPointerCapture(id) { this.captured.add(id); }
  hasPointerCapture(id) { return this.captured.has(id); }
  releasePointerCapture(id) { this.captured.delete(id); }
}

function fixture(saved = null, broken = false, docked = 'left') {
  const values = new Map(saved === null ? [] : [['width', saved]]);
  const storage = {
    getItem(key) { if (broken) throw new Error('Storage unavailable'); return values.get(key) ?? null; },
    setItem(key,value) { if (broken) throw new Error('Storage unavailable'); values.set(key,value); },
    removeItem(key) { if (broken) throw new Error('Storage unavailable'); values.delete(key); },
  };
  const classes = new Set();
  const root = new EventTarget();
  Object.defineProperty(root,'localStorage',{get() { if (broken) throw new Error('Storage unavailable'); return storage; }});
  const body = { classList: { add: key=>classes.add(key), remove: key=>classes.delete(key) } };
  vm.runInNewContext(fs.readFileSync(file,'utf8'),{ window:root, document:{body} },{filename:file});
  const handle = new Handle();
  let width=280, max=520, enabled=true, changes=0;
  const options = {handle,key:'width',measure:()=>width,limits:()=>[220,max],apply:value=>{width=value??280;},enabled:()=>enabled,docked,onChange:()=>{changes++;}};
  const control=root.leoPaneResize.create(options);
  const emit=(type,values={},target=handle)=>{const event=new Event(type,{cancelable:true});Object.assign(event,values);target.dispatchEvent(event);return event;};
  return {root,handle,control,classes,values,emit,options,width:()=>width,changes:()=>changes,limit:value=>{max=value;},enable:value=>{enabled=value;}};
}

const down = f=>f.emit('pointerdown',{button:0,pointerId:1,clientX:280});
const move = (f,extra={})=>f.emit('pointermove',{buttons:1,pointerId:1,clientX:350,...extra});

test('saved widths clamp to the current room without overwriting the preference',()=>{
  const f=fixture('480');
  assert.equal(f.width(),480);
  f.limit(360);f.emit('resize',{},f.root);
  assert.equal(f.width(),360);
  assert.equal(f.values.get('width'),'480');
  assert.equal(f.handle.attributes.get('aria-valuenow'),'360');
  f.limit(520);f.control.fit();assert.equal(f.width(),480);
  assert.equal(f.handle.attributes.get('aria-valuemin'),'220');
  assert.equal(f.handle.attributes.get('aria-valuemax'),'520');
});

test('invalid and missing saved widths use the default, and small widths clamp',()=>{
  for(const value of [null,'Infinity','NaN','junk','0','-20'])assert.equal(fixture(value).width(),280);
  assert.equal(fixture('100').width(),220);
});

test('the default width also fits tight layouts without becoming a saved preference',()=>{
  const f=fixture();
  f.limit(240);f.control.fit();assert.equal(f.width(),240);
  assert.equal(f.handle.attributes.get('aria-valuenow'),'240');
  assert.equal(f.values.has('width'),false);
  f.limit(520);f.control.fit();assert.equal(f.width(),280);
});

test('keyboard steps, maximum, reset and unrelated keys have predictable effects',()=>{
  const f=fixture();
  assert.equal(f.emit('keydown',{key:'ArrowRight'}).defaultPrevented,true);
  assert.equal(f.width(),312);
  f.emit('keydown',{key:'ArrowLeft',shiftKey:true});assert.equal(f.width(),220);
  f.emit('keydown',{key:'ArrowRight',shiftKey:true});assert.equal(f.width(),316);
  f.emit('keydown',{key:'End'});assert.equal(f.width(),520);
  f.emit('keydown',{key:'Home'});assert.equal(f.width(),280);assert.equal(f.values.has('width'),false);
  assert.equal(f.emit('keydown',{key:'Enter'}).defaultPrevented,false);
  f.enable(false);assert.equal(f.emit('keydown',{key:'ArrowRight'}).defaultPrevented,false);
  assert.equal(f.width(),280);
});

test('drag termination releases capture, removes the cursor and saves the width',()=>{
  for(const ending of ['pointerup','pointercancel','lostpointercapture','blur','buttons','hidden','capture']) {
    const f=fixture();down(f);move(f);
    assert.equal(f.width(),350);
    assert.equal(f.classes.has('pane-resizing'),true);
    if(ending==='blur')f.emit('blur',{},f.root);
    else if(ending==='buttons')move(f,{buttons:0});
    else if(ending==='hidden'){f.enable(false);f.control.fit();}
    else if(ending==='capture'){f.handle.releasePointerCapture(1);move(f);}
    else f.emit(ending,{pointerId:1,clientX:350});
    assert.equal(f.classes.has('pane-resizing'),false,ending);
    assert.equal(f.handle.hasPointerCapture(1),false,ending);
    assert.equal(f.values.get('width'),'350',ending);
    move(f,{clientX:400});assert.equal(f.width(),350,ending);
  }
});

test('unrelated pointers and mouse buttons cannot start or finish a drag',()=>{
  const f=fixture();move(f);assert.equal(f.width(),280);
  f.emit('pointerdown',{button:2,pointerId:1,clientX:280});assert.equal(f.classes.size,0);
  f.enable(false);down(f);assert.equal(f.classes.size,0);f.enable(true);
  down(f);move(f,{pointerId:2});assert.equal(f.width(),280);
  f.emit('pointerup',{pointerId:2,clientX:280});assert.equal(f.classes.has('pane-resizing'),true);
  move(f);assert.equal(f.changes(),1);f.control.stop();f.control.stop();
});

test('double tapping a handle resets its saved width',()=>{
  const f=fixture('400');
  down(f);f.emit('pointerup',{pointerId:1,clientX:280});down(f);
  assert.equal(f.width(),280);assert.equal(f.values.has('width'),false);
  assert.equal(f.classes.has('pane-resizing'),false);
});

test('starting another resize ends the first, and disabled drags end on movement',()=>{
  const f=fixture();down(f);
  const second=new Handle();
  const control=f.root.leoPaneResize.create({...f.options,handle:second});
  f.emit('pointerdown',{button:0,pointerId:3,clientX:280},second);
  assert.equal(f.handle.hasPointerCapture(1),false);
  control.stop();assert.equal(f.classes.has('pane-resizing'),false);
  down(f);f.enable(false);move(f);assert.equal(f.classes.has('pane-resizing'),false);
});

test('blocked storage does not prevent resizing or resetting',()=>{
  const f=fixture(null,true);
  f.emit('keydown',{key:'ArrowRight'});assert.equal(f.width(),312);
  f.emit('keydown',{key:'Home'});assert.equal(f.width(),280);
});

test('a right-docked pane grows when dragged left and with ArrowLeft',()=>{
  const f=fixture(null,false,'right');
  down(f);move(f,{clientX:230});assert.equal(f.width(),330);
  f.emit('pointerup',{pointerId:1,clientX:230});
  assert.equal(f.values.get('width'),'330');
  f.emit('keydown',{key:'ArrowLeft'});assert.equal(f.width(),362);
  f.emit('keydown',{key:'ArrowRight',shiftKey:true});assert.equal(f.width(),266);
  f.emit('keydown',{key:'End'});assert.equal(f.width(),520);
  f.emit('keydown',{key:'Home'});assert.equal(f.width(),280);
});
