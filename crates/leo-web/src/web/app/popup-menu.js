let popup = null;
let armed = null;

function closePopup(focus = true) {
  if (!popup) return;
  const { box, owner, marked } = popup;
  popup = null;
  box.inert = true;
  box.setAttribute('aria-hidden', 'true');
  box.className = 'popup-menu-gone';
  fold(box, () => box.remove());
  if (marked) owner.hasAttribute('aria-haspopup') ? owner.setAttribute('aria-expanded', 'false') : owner.removeAttribute('aria-expanded');
  if (focus && owner.isConnected) owner.focus({ preventScroll: true });
}

function popupRow(row, index) {
  if (row.line) return '<hr>';
  if (row.note) return `<p>${esc(row.note)}</p>`;
  const action = row.action ? ` data-action="${row.action}"${row.dir === undefined ? '' : ` data-dir="${esc(row.dir)}"`}` : '';
  const radio = row.checked === undefined ? 'role="menuitem"' : `role="${row.toggle ? 'menuitemcheckbox' : 'menuitemradio'}" aria-checked="${row.checked}"`;
  return `<button type="button" ${radio} data-row="${index}"${action}${row.danger ? ' class="danger"' : ''}${row.off ? ' disabled' : ''}>${row.icon || ''}<span class="grow">${esc(row.text)}</span>${row.after || (row.checked ? ICON.check : '')}</button>`;
}

function popupMenu(owner, { label, rows, point = null, find = '', page = null, more = false, loaded = 0 }) {
  const again = !point && popup && !popup.point && popup.owner === owner && popup.label === label;
  closePopup(again);
  if (again) return;
  const box = document.createElement('div');
  box.className = 'popup-menu';
  box.setAttribute('role', 'menu');
  box.setAttribute('aria-label', label);
  box.innerHTML = `${find ? `<input placeholder="${esc(find)}" aria-label="${esc(find)}" autocomplete="off" spellcheck="false">` : ''}<div class="popup-rows"></div>`;
  const input = box.querySelector('input');
  const list = box.querySelector('.popup-rows');
  document.body.appendChild(box);
  let shown = rows, ask = 0, at = loaded, waiting = false, timer = 0;
  const draw = (items, tail = false) => {
    shown = tail ? [...items, { text: 'Show more', more: true }] : items;
    list.innerHTML = shown.map(popupRow).join('');
  };
  const answer = (reply) => {
    at = reply.loaded;
    draw(reply.rows.length ? reply.rows : [{ note: 'Nothing matches' }], reply.more);
    place();
  };
  const fetch = async (query, offset, mine) => {
    const reply = await page(query, offset);
    if (mine === ask) answer(reply);
  };
  const place = () => {
    const bounds = owner.getBoundingClientRect();
    box.style.left = `${Math.max(8, Math.min(point?.x ?? bounds.left, innerWidth - box.offsetWidth - 8))}px`;
    box.style.top = `${Math.max(8, Math.min(point?.y ?? bounds.bottom + 4, innerHeight - box.offsetHeight - 8))}px`;
  };
  const first = () => (input || list.querySelector('button:not(:disabled)'))?.focus();
  const marked = owner.hasAttribute('aria-haspopup') || owner.matches('button:not([aria-expanded])');
  if (marked) owner.setAttribute('aria-expanded', 'true');
  draw(rows, more);
  place();
  popup = { box, owner, point, label, marked };
  unfold(box, point || owner);
  first();
  input?.addEventListener('input', () => {
    if (page) {
      clearTimeout(timer);
      const mine = ++ask;
      const query = input.value.trim();
      waiting = !!query;
      if (!query) { at = loaded; draw(rows, more); place(); return; }
      timer = setTimeout(() => fetch(query, 0, mine).then(() => { if (mine === ask) waiting = false; }, (e) => { if (mine === ask) waiting = false; fail(e); }), 160);
      return;
    }
    const wanted = input.value.trim().toLowerCase();
    const found = rows.filter((row) => row.text.toLowerCase().includes(wanted));
    draw(found.length ? found : [{ note: 'Nothing matches' }]);
  });
  box.addEventListener('click', (e) => {
    const button = e.target.closest('[data-row]');
    if (!button || button.disabled) return;
    const row = shown[button.dataset.row];
    if (row.more) {
      const mine = ++ask, query = input?.value.trim() || '', index = Number(button.dataset.row);
      fetch(query, at, mine).then(() => list.querySelectorAll('button')[index]?.focus(), fail);
      return;
    }
    if (row.next) { draw(row.next); place(); first(); return; }
    closePopup();
    if (row.run) Promise.resolve().then(row.run).catch(fail);
  });
  box.addEventListener('mousedown', (e) => { if (e.target !== input) e.preventDefault(); });
  box.addEventListener('contextmenu', (e) => e.preventDefault());
  box.addEventListener('keydown', (e) => {
    const buttons = [...list.querySelectorAll('button:not(:disabled)')];
    const stops = input ? [input, ...buttons] : buttons;
    const at = stops.indexOf(document.activeElement);
    const typing = e.target === input;
    let next;
    if (e.key === 'Enter' && typing && !e.isComposing) { e.preventDefault(); if (!waiting) buttons[0]?.click(); return; }
    if (e.key === 'ArrowDown') next = (at + 1) % stops.length;
    else if (e.key === 'ArrowUp') next = (at + stops.length - 1) % stops.length;
    else if (e.key === 'Home' && !typing) next = 0;
    else if (e.key === 'End' && !typing) next = stops.length - 1;
    else if (e.key === 'Escape' || e.key === 'Tab') { closePopup(); e.preventDefault(); e.stopPropagation(); return; }
    if (next !== undefined) { e.preventDefault(); stops[next]?.focus(); }
  });
}

document.addEventListener('pointerdown', (event) => {
  armed = null;
  if (!popup || popup.box.contains(event.target)) return;
  if (event.button === 0 && !popup.point && popup.owner.contains(event.target)) armed = popup;
  else closePopup(false);
});
document.addEventListener('click', (event) => {
  const kept = armed;
  armed = null;
  if (popup?.point && popup.owner.contains(event.target)) {
    event.preventDefault();
    event.stopImmediatePropagation();
    return;
  }
  if (kept && kept.owner.contains(event.target)) setTimeout(() => popup === kept && closePopup(false));
}, true);
addEventListener('hashchange', () => closePopup(false));

function closeOnOutsidePress(inside, leave) {
  const keep = `${inside}, .scrim, .popup-menu, .select-menu, .toast`;
  let armed = false;
  document.addEventListener('pointerdown', (e) => {
    armed = e.button === 0 && e.isPrimary && !e.target.closest?.(keep);
  });
  document.addEventListener('click', (e) => {
    const pressed = armed && e.detail > 0 && !e.target.closest?.(keep);
    armed = false;
    if (!pressed) return;
    Promise.resolve(leave()).then((opener) => {
      if (opener && (!document.activeElement || document.activeElement === document.body)) opener.focus({ preventScroll: true });
    }, fail);
  });
}
