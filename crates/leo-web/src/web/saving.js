(function (root) {
  'use strict';

  const PREFIX = 'leo-draft-v1:';
  const copy = (value) => JSON.parse(JSON.stringify(value));
  const draftId = () => Array.from(root.crypto.getRandomValues(new Uint32Array(4)), (n) => n.toString(16).padStart(8, '0')).join('');

  function create({ api, storage, mark = () => {}, recovered = () => {}, conflict = () => {}, created = () => {}, storageError = () => {}, retryable = () => false, schedule = setTimeout, cancel = clearTimeout }) {
    const pending = new Map();
    let warned = false;

    function stored() {
      const found = [];
      try {
        for (let i = 0; i < storage.length; i++) {
          const key = storage.key(i);
          if (!key || !key.startsWith(PREFIX)) continue;
          try {
            const value = JSON.parse(storage.getItem(key));
            if (value && value.note && value.edit && typeof value.edit.title === 'string' && typeof value.edit.body === 'string' && Array.isArray(value.edit.tags) && value.edit.tags.every((tag) => typeof tag === 'string') && typeof value.note.directory === 'string' && (value.note.id === null || typeof value.note.id === 'string')) {
              found.push({ key, ...value });
            }
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
        edit: copy(draft ? draft.edit : { title: note.title, body: note.body, tags: note.tags }),
        dirty: Boolean(draft), revision: 0, timer: null, saving: null, persisted: Boolean(draft),
      };
      if (s.dirty) pending.set(key, s);
      return s;
    }

    function persist(s) {
      pending.set(s.key, s);
      try {
        storage.setItem(s.key, JSON.stringify({ note: s.note, edit: s.edit }));
        s.persisted = true;
      } catch (error) {
        s.persisted = false;
        if (!warned) { warned = true; storageError(error); }
      }
    }

    function remove(s) {
      pending.delete(s.key);
      try { storage.removeItem(s.key); } catch (_) {}
    }

    function changed(s, edit) {
      s.edit = copy(edit);
      s.revision++;
      s.dirty = true;
      persist(s);
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
        s.edit.title = `${s.edit.title} (conflict from leo)`;
        conflict(s, note);
        return note;
      }
    }

    function flush(s) {
      if (!s) return Promise.resolve();
      cancel(s.timer);
      if (s.saving) return s.saving;
      if (!s.dirty) return Promise.resolve();
      s.saving = (async () => {
        while (s.dirty) {
          const edit = copy(s.edit);
          if (!s.note.id && !edit.title.trim() && !edit.body.trim() && !edit.tags.length) {
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
              s.edit = { title: s.note.title, body: s.note.body, tags: [...s.note.tags] };
              remove(s);
              mark(s, 'Saved');
            }
          } catch (error) {
            if (s.discarded) return;
            s.dirty = true;
            persist(s);
            mark(s, s.persisted ? 'Not saved to leo · draft kept on this browser' : 'Not saved · keep this page open');
            if (retryable(error)) s.timer = schedule(() => flush(s).catch(() => {}), 4000);
            throw error;
          }
        }
      })().finally(() => { s.saving = null; });
      return s.saving;
    }

    const drafts = stored();
    for (const draft of drafts) session(draft.note, draft);
    if (drafts.length) recovered(drafts.length);

    return {
      open: (note) => {
        const existing = [...pending.values()].find((s) => note.id && s.note.id === note.id);
        return existing || session(note);
      },
      get: (key) => pending.get(key),
      drafts: () => [...pending.values()].map((s) => ({ key: s.key, note: copy(s.note), edit: copy(s.edit) })),
      changed, flush,
      unsaved: () => pending.size > 0,
      discardAll: () => {
        const all = [...pending.values()];
        for (const s of all) {
          if (s.timer) cancel(s.timer);
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
