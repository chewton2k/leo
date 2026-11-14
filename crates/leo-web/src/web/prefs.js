(function (root) {
  'use strict';

  const NAME = /^leo-[a-z0-9-]{1,48}$/;
  const CONTENT = ['leo-chat-v1', 'leo-search'];
  const MOST_VALUE = 16 * 1024;
  const WAIT = 400;
  const seed = root.__LEO_PREFS__;
  let local;
  try {
    local = root.localStorage;
  } catch (e) {
    return;
  }
  if (!local || !seed || typeof seed !== 'object') return;
  const proto = root.Storage.prototype;
  const { setItem, removeItem } = proto;
  const pending = new Map();
  let timer = 0;

  const kept = (key) => NAME.test(key) && !key.startsWith('leo-draft') && !CONTENT.includes(key);
  const fits = (value) => new TextEncoder().encode(value).length <= MOST_VALUE;

  function send(leaving) {
    root.clearTimeout(timer);
    timer = 0;
    if (!pending.size) return;
    const body = { set: {}, remove: [] };
    for (const [key, value] of pending) {
      if (value === null) body.remove.push(key);
      else body.set[key] = value;
    }
    const sent = [...pending];
    pending.clear();
    root.fetch('/api/ui-prefs', { method: 'PUT', headers: { 'content-type': 'application/json' }, body: JSON.stringify(body), keepalive: Boolean(leaving) }).catch(() => {
      for (const [key, value] of sent) if (!pending.has(key)) pending.set(key, value);
      if (pending.size && !timer) timer = root.setTimeout(send, WAIT * 10);
    });
  }

  function note(key, value) {
    if (!kept(key)) return;
    pending.set(key, value !== null && !fits(value) ? null : value);
    if (!timer) timer = root.setTimeout(send, WAIT);
  }

  try {
    const files = Object.keys(seed).filter((key) => kept(key) && typeof seed[key] === 'string');
    if (files.length) {
      for (const key of files) if (local.getItem(key) !== seed[key]) setItem.call(local, key, seed[key]);
    } else {
      for (let i = 0; i < local.length; i++) note(local.key(i), local.getItem(local.key(i)));
    }
  } catch (e) {}

  proto.setItem = function (key, value) {
    setItem.call(this, key, value);
    if (this === local) note(String(key), String(value));
  };
  proto.removeItem = function (key) {
    removeItem.call(this, key);
    if (this === local) note(String(key), null);
  };
  root.addEventListener('pagehide', () => send(true));
  root.document.addEventListener('visibilitychange', () => {
    if (root.document.visibilityState === 'hidden') send(true);
  });
})(window);
