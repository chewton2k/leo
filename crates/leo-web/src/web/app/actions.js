const actions = {
  'app-command': (el) => appChange({action:el.dataset.appAction}),
  'health-refresh': loadHealth,
  'onboarding-next': finishOnboarding,
  'onboarding-model': onboardingModel,
  'onboarding-skip': skipOnboarding,
  'onboarding-back': onboardingBack,
  'onboarding-pick': pickOnboardingModel,
  'onboarding-models': showOnboardingModels,
  'onboarding-writer': pickOnboardingWriter,
  'onboarding-use': onboardingUse,
  'onboarding-sign-in': onboardingSignIn,
  'writing-sign-in': writingSignIn,
  'shortcut-record': (el) => shortcutRecord(el.dataset.name),
  'shortcut-reset': (el) => shortcutReset(el.dataset.name),
  'dictation-record': () => dictationChange({action:'record_shortcut'}),
  'dictation-original': toggleDictationOriginal,
  'dictation-clear': clearDictationsAsk, 'dictation-clear-now': clearDictationsNow,
  'dictation-copy': (el) => dictationChange({action:'copy',id:Number(el.dataset.id)}).then(() => pulse(el)),
  'dictation-edit': startDictationEdit,
  'dictation-add-word': addDictationWord,
  'speech-filter': filterSpeech,
  'speech-model': changeSpeechModel,
  'ollama-pick': ollamaPick,
  'ollama-pull': ollamaPull,
  'ollama-cancel': () => ollamaChange({ action: 'cancel' }),
  back,
  home: () => go('#/'),
  'side-fold': foldSide,
  'toggle-picker': togglePicker,
  'ask-selection': () => askAboutSelection(),
  calendar: () => {
    closeSheet();
    go('#/calendar');
  },
  boards: () => {
    closeSheet();
    go('#/boards');
  },
  'kanban-new-board': () => kanbanNewBoard(),
  'kanban-board-menu': kanbanBoardMenu,
  'kanban-column-menu': kanbanColumnMenu,
  'kanban-add-card': (el) => kanbanComposer(el.dataset.col),
  'kanban-add-column': () => kanbanAddColumn(),
  'kanban-open-card': (el) => kanbanCardPanel(el.dataset.card),
  'cal-pick': (el) => {
    cal.selected = el.dataset.day;
    if (el.dataset.to) calSetView(el.dataset.to);
    calGo();
  },
  'cal-prev': () => calStep(-1),
  'cal-next': () => calStep(1),
  'cal-today': () => {
    cal.selected = S.dayKey(new Date());
    calGo();
  },
  'cal-view': (el) => calSetView(el.dataset.view),
  'cal-mini': (el) => {
    const at = S.fromKey(cal.selected);
    const m = new Date(at.getFullYear(), at.getMonth() + Number(el.dataset.step), 1);
    cal.selected = S.dayKey(m);
    calGo();
  },
  'cal-open': (el) => calOpenPeek(el.dataset.kind, el.dataset.id, el.dataset.on || null),
  'cal-peek-close': () => calClosePeek(),
  'cal-retry': () => {
    cal.polls = 0;
    return api('/api/calendar/refresh', { method: 'POST' }).catch(() => null).then(() => loadSchedule(seq));
  },
  'cal-done': () => {
    cal.showDone = !cal.showDone;
    try {
      localStorage.setItem(CAL_DONE_KEY, cal.showDone ? '0' : '1');
    } catch (e) {
      cal.showDone = Boolean(cal.showDone);
    }
    drawCalendar();
  },
  'task-new': (el) => newTaskSheet(el.dataset.day || null),
  'task-open': (el) => taskSheet(el.dataset.id, el.dataset.on || null),
  'task-open-here': (el) => openTaskAnywhere(el.dataset.id, el.dataset.on || null),
  'task-done': (el) => {
    const id = el.dataset.id;
    const on = el.dataset.on || null;
    const task = calFind(id);
    const done = el.dataset.set ? el.dataset.set === '1' : !(task && taskDoneOn(task, on));
    if (el.dataset.set) closeSheet();
    return setTaskDone(id, done, on).then(() => {
      for (const box of document.querySelectorAll(`.cal-check[data-id="${CSS.escape(id)}"]${on ? `[data-on="${CSS.escape(on)}"]` : ''}`)) box.checked = done;
    });
  },
  'task-date': (el) => dateSheet(el.dataset.id),
  'task-google': (el) => addToGoogle(el.dataset.id),
  'task-delete': (el) => deleteTask(el.dataset.id),
  'task-promote': (el) => promoteTask(el.dataset.line),
  'note-tasks': () => noteTasksSheet(),
  'source-close': closeSource,
  'source-file': (el) => openSource(el.dataset.id, el.dataset.name),
  'set-model': (el) => changeSetting({ set: 'model', task: el.dataset.task, value: el.dataset.model }),
  'set-effort': (el) => changeSetting({ set: 'effort', value: el.dataset.effort === 'default' ? '' : el.dataset.effort }),
  'side-twist': (el) => twistFolder(el.dataset.dir, el),
  'side-sort': sideSortMenu,
  'side-flip': () => sideOrder({ flip: !side.flip }),
  'side-notes': (el) => loadSideNotes(el.dataset.dir, Number(el.dataset.offset || 0), true),
  'open-title': (el) => openTitle(el.dataset.title || ''),
  'toggle-search': () => toggleQuickLeo(),
  menu,
  'calendar-add': () => addCalendar().catch(fail),
  'calendar-remove': (el) => removeCalendar(el).catch(fail),
  'calendar-more': () => {
    state.calendarAdding = true;
    drawSettings(state.settings);
    const box = $('#calendar-link');
    if (box) box.focus();
  },
  'calendar-cancel': () => {
    state.calendarAdding = false;
    drawSettings(state.settings);
  },
  'note-sources': () => showRecordingSources(window.getSelection().toString().trim().slice(0,200)).catch(fail),
  'source-save': () => saveSource().catch(fail),
  'source-regenerate': () => regenerateSource().catch(fail),
  'source-back': () => drawRecordingSources(),
  'source-apply': () => applyGenerated().catch(fail),
  'rec-finish': () => recorder.finishAvailable().catch(fail),
  'rec-recover': (el) => recorder.recover(el.dataset.id).catch(fail),
  close: () => closeSheet(),
  'open-folder': openFolder,
  'open-note': openExplorerNote,
  'explorer-menu': explorerMenu,
  folders: showFolders,
  'pin-card': pinCard,
  'theme-pick': (el) => { pickTheme(el); pulse(el, { lift: 1.02 }); },
  'segment-pick': (el) => pickSegment(el),
  'felix-pick': (el) => { pickFelix(el); pulse(el, { lift: 1.02 }); },
  'width-pick': (el) => { pickWidth(el); pulse(el, { lift: 1.02 }); },
  'text-size-pick': (el) => { pickTextSize(el); pulse(el, { lift: 1.02 }); },
  'letters-pick': (el) => { pickLetters(el); pulse(el, { lift: 1.02 }); },
  'setting-reset': (el) => resetSetting(el),
  'usage': () => showUsage(),
  'quick-leo': () => quickLeo(),
  'quick-run': (el) => quickGo(Number(el.dataset.at)),
  'quick-done': (el) => quickDone(el),
  new: (el) => {
    const dir = creationDir(el.dataset.dir);
    closeSheet(true);
    go(dir ? `#/new/${enc(dir)}` : '#/new');
  },
  'search-ask': (el) => {
    if (!chat.askNew(el.dataset.query)) toast('Felix is finishing another answer. Try again in a moment.', { bad: true });
  },
  'search-new': async (el) => {
    el.disabled = true;
    try {
      const note = await api('/api/notes', { method: 'POST', body: { title: Array.from(el.dataset.query).slice(0, 120).join(''), body: '', directory: creationDir() } });
      go(noteHash(note.id));
    } finally {
      el.disabled = false;
    }
  },
  'search-retry': (el) => showSearch(el.dataset.query),
  'new-folder': newFolder,
  'create-folder': createFolder,
  settings: () => {
    closeSheet();
    go('#/settings');
  },
  keeping: () => go('#/settings/keeping'),
  'keep-now': () => {
    const pending = state.keepPending;
    state.keepPending = null;
    closeSheet();
    if (pending) saveKeep(pending.next, pending.goes).catch(fail);
  },
  'keep-cancel': () => closeSheet(),
  'tasks-restore-pick': () => $('#tasks-restore')?.click(),
  'drafts-retry': () => draftsRetry().catch(fail),
  'drafts-discard-ask': (el) => draftsDiscardAsk(el),
  'drafts-discard': () => draftsDiscard(),
  'tasks-restore-go': () => tasksRestoreGo().catch(fail),
  'set-key': (el) => saveKey(el.dataset.account),
  'remove-key': confirmRemoveKey,
  'test-ai': testAi,
  'note-picture': () => choosePictures(),
  'view-original': (el) => {
    el.focus({ preventScroll: true });
    return viewOriginal(el.dataset.id, el.dataset.name);
  },
  'diagram-expand': (el) => {
    el.focus({ preventScroll: true });
    expandDiagrams(el);
  },
  'viewer-zoom': (el) => setViewerZoom(Number(el.dataset.step) === 0 ? 1 : state.viewerZoom * (Number(el.dataset.step) > 0 ? 1.5 : 1 / 1.5)),
  record: (el) => {
    closeSheet();
    go(el.dataset.dir ? `#/record/${enc(el.dataset.dir)}` : '#/record');
  },
  'rec-start': () => recorder.begin().catch(fail),
  'rec-pause': () => recorder.pause().catch(fail),
  'rec-stop': () => recorder.stop().then(checkActivity).catch(fail),
  'activity-open': (el) => (el.dataset.href.startsWith('#combine/') ? reviewCombine(el.dataset.href.slice('#combine/'.length)).catch(fail) : go(el.dataset.href)),
  'activity-fold': () => {
    activity.folded = !activity.folded;
    drawActivity();
  },
  'rec-point': () => recorder.point(),
  'rec-dismiss': () => recorder.dismiss(),
  'rec-discard': (el) => discardRecordingAsk(el),
  'rec-discard-now': (el) => discardRecordingNow(el),
  'rec-again': () => recorder.again(),
  upload: (el) => uploadSheet(undefined, el.dataset.dir, { add: el.dataset.add === 'yes' }).catch(fail),
  'upload-go': () => uploadGo().catch(fail),
  'upload-drop': (el) => {
    picked.splice(Number(el.dataset.i), 1);
    drawPicked();
  },
  chat: async () => {
    if (chat.isOpen()) return chat.toggle(false);
    chat.toggle(true);
  },
  trash: () => {
    closeSheet();
    go('#/trash');
  },
  refresh: () => {
    closeSheet();
    saving.retry();
    route();
  },
  move: (el) => moveSheet(el),
  share: (el) => shareMenu(el),
  delete: () => trashNote(),
  restore: (el) => restore(el.dataset.id, false),
  'trash-select': () => {
    state.selecting = !state.selecting;
    state.picked = new Set();
    drawTrash();
  },
  'trash-empty': () => trashAsk([], true),
  'trash-forget': (el) => trashAsk([el.dataset.id], false),
  'trash-delete-picked': () => state.picked.size && trashAsk([...state.picked], false),
  'trash-delete-now': () => trashDeleteNow().catch(fail),
  'trash-restore-picked': () => trashRestorePicked().catch(fail),
};
