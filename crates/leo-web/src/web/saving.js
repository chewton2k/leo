(function (root) {
  'use strict';

  const PREFIX = 'leo-draft-v1:';
  const copy = (value) => JSON.parse(JSON.stringify(value));
  const copyEdit = (e) => ({ title: e.title, body: e.body });
  const DRAFT_EVERY = 250;
  const SETTLE_WITHIN = 5000;
  const SMALL_DRAFT = 262144;
  const LARGE_DB = 'leo-drafts';
  const LARGE_STORE = 'drafts';
  const valid = (value) => Boolean(value && value.note && value.edit && typeof value.edit.title === 'string' && typeof value.edit.body === 'string' && typeof value.note.directory === 'string' && (value.note.id === null || typeof value.note.id === 'string'));
  const draftId = () => Array.from(root.crypto.getRandomValues(new Uint32Array(4)), (n) => n.toString(16).padStart(8, '0')).join('');

  function create({ api, storage, mark = () => {}, recovered = () => {}, conflict = () => {}, created = () => {}, storageError = () => {}, retryable = () => false, schedule = setTimeout, cancel = clearTimeout, now = Date.now, indexedDB: idb = root.indexedDB }) {
    const pending = new Map();
    let warned = false;
    let opened = null;
    let line = Promise.resolve();
    const largeKeys = new Set();

    function database() {
      opened ||= new Promise((resolve, reject) => {
        if (!idb) throw new Error('This browser has no IndexedDB.');
        const request = idb.open(LARGE_DB, 1);
        request.onupgradeneeded = () => request.result.createObjectStore(LARGE_STORE);
        request.onsuccess = () => resolve(request.result);
        request.onerror = () => reject(request.error || new Error('The draft store did not open.'));
      });
      return opened;
    }

    function large(mode, work) {
      const run = () => database().then((db) => new Promise((resolve, reject) => {
        const tx = db.transaction(LARGE_STORE, mode);
        const request = work(tx.objectStore(LARGE_STORE));
        tx.oncomplete = () => resolve(request && request.result);
        tx.onerror = tx.onabort = () => reject(tx.error || new Error('The draft store did not finish.'));
      }));
      line = line.catch(() => {}).then(run);
      return line;
    }

    function forgetLarge(key) {
      largeKeys.delete(key);
      large('readwrite', (store) => { store.delete(key); }).catch(() => {});
    }

    function warn(s, error) {
      s.persisted = false;
      if (!warned) { warned = true; storageError(error); }
    }

    function stored() {
      const found = [];
      try {
        for (let i = 0; i < storage.length; i++) {
          const key = storage.key(i);
          if (!key || !key.startsWith(PREFIX)) continue;
          try {
            const value = JSON.parse(storage.getItem(key));
            if (valid(value)) found.push({ key, ...value });
          } catch (_) {}
        }
      } catch (_) {}
      return found;
    }

    function session(note, draft = null) {
      const key = draft ? draft.key : PREFIX + (note.id || 'new:' + draftId());
      if (pending.has(key)) return pending.get(key);
      const s = {
        key, note: copy(draft ? draft.note : note),
        edit: copyEdit(draft ? draft.edit : note),
        dirty: Boolean(draft), revision: 0, timer: null, saving: null, persisted: Boolean(draft),
      };
      if (s.dirty) pending.set(key, s);
      return s;
    }

    function writeSmall(s, record, revision) {
      try {
        storage.setItem(s.key, JSON.stringify(record));
        s.persisted = true;
        s.durableRevision = revision;
        return true;
      } catch (error) {
        warn(s, error);
        return false;
      }
    }

    function writeLarge(s, record, revision) {
      const current = () => pending.get(s.key) === s && !s.discarded;
      s.writing = large('readwrite', (store) => { store.put(record, s.key); }).then(() => {
        largeKeys.add(s.key);
        if (!current()) {
          if (!pending.has(s.key)) forgetLarge(s.key);
          return;
        }
        s.persisted = true;
        s.durableRevision = revision;
        try { storage.removeItem(s.key); } catch (_) {}
      }, () => {
        if (current() && !writeSmall(s, record, revision)) mark(s, 'Not saved · keep this page open');
      }).then(() => {
        s.writing = null;
        if (current() && s.persistDue) s.persistTimer = schedule(() => { if (s.persistDue && pending.get(s.key) === s) persist(s); }, Math.max(0, s.persistedAt + DRAFT_EVERY - now()));
      });
    }

    function persist(s) {
      pending.set(s.key, s);
      if (s.writing) { s.persistDue = true; return; }
      s.persistDue = false;
      s.persistedAt = now();
      const edit = copyEdit(s.edit);
      const record = { note: { ...s.note, body: '' }, edit, at: s.persistedAt };
      if (edit.title.length + edit.body.length > SMALL_DRAFT) return writeLarge(s, record, s.revision);
      if (writeSmall(s, record, s.revision) && largeKeys.has(s.key)) forgetLarge(s.key);
    }

    function remove(s) {
      pending.delete(s.key);
      try { storage.removeItem(s.key); } catch (_) {}
      if (largeKeys.has(s.key)) forgetLarge(s.key);
    }

    function changed(s, edit) {
      s.edit = edit;
      s.revision++;
      s.dirty = true;
      pending.set(s.key, s);
      if (now() - (s.persistedAt || 0) >= DRAFT_EVERY) persist(s);
      else s.persistDue = true;
      mark(s, 'Editing…');
      cancel(s.timer);
      s.timer = schedule(() => flush(s).catch(() => {}), 700);
    }

    async function send(s, edit) {
      if (!s.note.id) {
        s.note.request ||= draftId();
        persist(s);
        const note = await api('/api/notes', { method: 'POST', body: { ...edit, directory: s.note.directory, request: s.note.request } });
        created(s, note);
        return note;
      }
      try {
        return await api(`/api/notes/${encodeURIComponent(s.note.id)}`, {
          method: 'PATCH', body: { ...edit, base: s.note.version },
        });
      } catch (error) {
        if (error.status !== 409) throw error;
        const title = `${edit.title} (conflict from leo)`;
        s.note.copyRequest ||= draftId();
        persist(s);
        const note = await api('/api/notes', { method: 'POST', body: { ...edit, title, directory: s.note.directory, request: s.note.copyRequest } });
        s.edit = { title: `${s.edit.title} (conflict from leo)`, body: s.edit.body };
        conflict(s, note);
        return note;
      }
    }

    function flush(s) {
      if (!s) return Promise.resolve();
      if (s.persistDue) persist(s);
      cancel(s.timer);
      if (s.saving) return s.saving;
      if (!s.dirty) return Promise.resolve();
      s.saving = (async () => {
        while (s.dirty) {
          const edit = copyEdit(s.edit);
          if (!s.note.id && !edit.title.trim() && !edit.body.trim()) {
            s.dirty = false;
            remove(s);
            return;
          }
          edit.title = edit.title.trim() || 'Untitled';
          const revision = s.revision;
          s.dirty = false;
          mark(s, 'Saving…');
          try {
            s.note = await send(s, edit);
            if (s.discarded) return;
            if (s.revision !== revision || s.dirty) {
              s.dirty = true;
              persist(s);
            } else {
              s.edit = { title: s.note.title, body: s.note.body };
              remove(s);
              mark(s, 'Saved');
            }
          } catch (error) {
            if (s.discarded) return;
            s.dirty = true;
            persist(s);
            if (s.writing) await s.writing;
            if (s.discarded) return;
            mark(s, s.persisted ? 'Not saved to leo · draft kept on this browser' : 'Not saved · keep this page open');
            if (retryable(error)) s.timer = schedule(() => flush(s).catch(() => {}), 4000);
            throw error;
          }
        }
      })().finally(() => { s.saving = null; });
      return s.saving;
    }

    const durable = (s) => !pending.has(s.key) || s.durableRevision === s.revision;

    async function settle(within = SETTLE_WITHIN) {
      const work = Promise.all([...pending.values()].map(async (s) => {
        for (let tries = 0; tries < 2 && !durable(s); tries++) {
          persist(s);
          if (s.writing) await s.writing;
        }
        if (!durable(s)) await flush(s).catch(() => {});
      })).then(() => [...pending.values()].every(durable));
      let timer;
      const late = new Promise((resolve) => { timer = schedule(() => resolve(false), within); });
      try {
        const done = await Promise.race([work, late]);
        if (!done) for (const s of pending.values()) if (!durable(s)) mark(s, 'Not saved · keep this page open');
        return done;
      } finally { cancel(timer); }
    }

    const drafts = stored();
    for (const draft of drafts) session(draft.note, draft);
    if (drafts.length) recovered(drafts.length);

    const ready = (idb ? large('readonly', (store) => {
      const keys = store.getAllKeys();
      const values = store.getAll();
      return { get result() { return keys.result.map((key, i) => ({ key, value: values.result[i] })); } };
    }) : Promise.resolve([])).then((records) => {
      let count = 0;
      for (const { key, value } of records) {
        if (typeof key !== 'string' || !key.startsWith(PREFIX) || !valid(value)) continue;
        largeKeys.add(key);
        const known = pending.get(key);
        if (known) {
          const local = drafts.find((d) => d.key === key);
          if ((value.at || 0) <= ((local && local.at) || 0)) continue;
          known.note = copy(value.note);
          known.edit = copyEdit(value.edit);
          try { storage.removeItem(key); } catch (_) {}
          continue;
        }
        session(value.note, { key, ...value });
        count++;
      }
      if (count) recovered(count);
    }, () => {});

    return {
      open: (note) => {
        const existing = [...pending.values()].find((s) => note.id && s.note.id === note.id);
        return existing || session(note);
      },
      get: (key) => pending.get(key),
      drafts: () => [...pending.values()].map((s) => ({ key: s.key, note: copy(s.note), edit: copyEdit(s.edit) })),
      changed, flush, settle, ready,
      unsaved: () => pending.size > 0,
      discardAll: () => {
        const all = [...pending.values()];
        for (const s of all) {
          if (s.timer) cancel(s.timer);
          if (s.persistTimer) cancel(s.persistTimer);
          s.timer = null;
          s.dirty = false;
          s.discarded = true;
          remove(s);
        }
        return all.length;
      },
      retry: () => Promise.allSettled([...pending.values()].map(flush)),
    };
  }

  root.leoSaving = { create };
})(typeof window === 'undefined' ? globalThis : window);
