(function (root) {
  'use strict';

  const KEY = 'leo-theme';
  const FELIX_KEY = 'leo-felix';
  const WIDTH_KEY = 'leo-note-width';
  const WIDTHS = ['comfortable', 'wide', 'full'];
  const READING = {
    size: { key: 'leo-text-size', attr: 'data-text-size', choices: ['small', 'medium', 'large'], fallback: 'medium' },
    letters: { key: 'leo-felix-letters', attr: 'data-felix-letters', choices: ['own', 'notes'], fallback: 'own' },
  };
  const CHOICES = ['system', 'light', 'dark'];
  const COLORS = { light: '#f6f5f1', dark: '#181715' };
  const media = root.matchMedia ? root.matchMedia('(prefers-color-scheme: dark)') : null;
  const listeners = new Set();
  let current = 'system';

  function stored() {
    try {
      const value = root.localStorage.getItem(KEY);
      return CHOICES.includes(value) ? value : 'system';
    } catch (e) {
      return 'system';
    }
  }

  function dark() {
    if (current === 'system') return Boolean(media && media.matches);
    return current === 'dark';
  }

  function tell() {
    const now = dark();
    for (const fn of listeners) {
      try {
        fn(now);
      } catch (e) {}
    }
  }

  function apply(choice) {
    current = CHOICES.includes(choice) ? choice : 'system';
    const html = root.document.documentElement;
    if (current === 'system') html.removeAttribute('data-theme');
    else html.setAttribute('data-theme', current);
    for (const meta of root.document.querySelectorAll('meta[name="theme-color"]')) {
      const own = /dark/.test(meta.getAttribute('media') || '') ? COLORS.dark : COLORS.light;
      meta.setAttribute('content', current === 'system' ? own : COLORS[current]);
    }
  }

  function set(choice) {
    apply(choice);
    try {
      if (current === 'system') root.localStorage.removeItem(KEY);
      else root.localStorage.setItem(KEY, current);
    } catch (e) {}
    tell();
  }

  function felix() {
    try {
      return root.localStorage.getItem(FELIX_KEY) === 'demon' ? 'demon' : 'cute';
    } catch (e) {
      return 'cute';
    }
  }

  function showFelix(kind) {
    const html = root.document.documentElement;
    if (kind === 'demon') html.setAttribute('data-felix', 'demon');
    else html.removeAttribute('data-felix');
  }

  function setFelix(kind) {
    const demon = kind === 'demon';
    showFelix(demon ? 'demon' : 'cute');
    try {
      if (demon) root.localStorage.setItem(FELIX_KEY, 'demon');
      else root.localStorage.removeItem(FELIX_KEY);
    } catch (e) {}
  }

  function noteWidth() {
    try {
      const value = root.localStorage.getItem(WIDTH_KEY);
      return WIDTHS.includes(value) ? value : 'wide';
    } catch (e) {
      return 'wide';
    }
  }

  function showWidth(width) {
    root.document.documentElement.setAttribute('data-note-width', WIDTHS.includes(width) ? width : 'wide');
  }

  function setNoteWidth(width) {
    const chosen = WIDTHS.includes(width) ? width : 'wide';
    showWidth(chosen);
    try {
      if (chosen === 'wide') root.localStorage.removeItem(WIDTH_KEY);
      else root.localStorage.setItem(WIDTH_KEY, chosen);
    } catch (e) {}
  }

  function reading(name) {
    const { key, choices, fallback } = READING[name];
    try {
      const value = root.localStorage.getItem(key);
      return choices.includes(value) ? value : fallback;
    } catch (e) {
      return fallback;
    }
  }

  function showReading(name, value) {
    const { attr, choices, fallback } = READING[name];
    root.document.documentElement.setAttribute(attr, choices.includes(value) ? value : fallback);
  }

  function setReading(name, value) {
    const { key, choices, fallback } = READING[name];
    const chosen = choices.includes(value) ? value : fallback;
    showReading(name, chosen);
    try {
      if (chosen === fallback) root.localStorage.removeItem(key);
      else root.localStorage.setItem(key, chosen);
    } catch (e) {}
  }

  function onChange(fn) {
    listeners.add(fn);
    return () => listeners.delete(fn);
  }

  apply(stored());
  showFelix(felix());
  showWidth(noteWidth());
  for (const name of Object.keys(READING)) showReading(name, reading(name));
  if (root.__LEO_APP__ === 'mac') root.document.documentElement.setAttribute('data-app', 'mac');
  if (media && media.addEventListener) {
    media.addEventListener('change', () => {
      if (current === 'system') tell();
    });
  }
  root.addEventListener('storage', (e) => {
    if (e.key === FELIX_KEY || e.key === null) showFelix(felix());
    if (e.key === WIDTH_KEY || e.key === null) showWidth(noteWidth());
    for (const name of Object.keys(READING)) if (e.key === READING[name].key || e.key === null) showReading(name, reading(name));
    if (e.key !== KEY && e.key !== null) return;
    const was = dark();
    apply(stored());
    if (dark() !== was) tell();
  });
  if (root.document.readyState === 'loading') {
    root.document.addEventListener('DOMContentLoaded', () => apply(current), { once: true });
  }

  root.leoTheme = { KEY, CHOICES, choice: () => current, dark, set, onChange, felix, setFelix, WIDTHS, noteWidth, setNoteWidth,
    TEXT_SIZES: READING.size.choices, textSize: () => reading('size'), setTextSize: (value) => setReading('size', value),
    LETTERINGS: READING.letters.choices, felixLetters: () => reading('letters'), setFelixLetters: (value) => setReading('letters', value) };
})(window);
