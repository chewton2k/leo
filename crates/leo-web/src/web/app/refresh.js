function appVisible() {
  return document.visibilityState !== 'hidden' && window.__LEO_VISIBLE__ !== false;
}

function refreshKey(node) {
  if (node.nodeType !== 1) return null;
  for (const name of ['id', 'data-refresh-key', 'data-kids', 'data-model-row', 'data-task-card', 'data-settings-section', 'data-dictation', 'data-key-input', 'data-app-pref', 'data-keep', 'data-export', 'data-trash-all']) {
    if (node.hasAttribute(name)) return `${node.nodeName}:${name}:${node.getAttribute(name)}`;
  }
  if (node.hasAttribute('data-trash-pick')) return JSON.stringify([node.nodeName, node.getAttribute('data-trash-pick'), node.getAttribute('value')]);
  if (node.hasAttribute('data-action') || node.hasAttribute('data-set')) {
    return JSON.stringify([node.nodeName, ...[...node.attributes].filter((attr) => attr.name.startsWith('data-') && !['data-title', 'data-name', 'data-menu'].includes(attr.name)).map((attr) => [attr.name, attr.value]).sort(([a], [b]) => a.localeCompare(b))]);
  }
  if (node.hasAttribute('data-area')) return `${node.nodeName}:area:${node.getAttribute('data-area')}`;
  if (node.matches('.segmented[aria-label],.choice-cards[aria-label]')) return `segment:${node.getAttribute('aria-label')}`;
  if (node.matches('a[href]')) return `link:${node.getAttribute('href')}`;
  return null;
}

function refreshChildren(box, desired) {
  const previous = [...box.childNodes];
  const keyed = new Map(), unkeyed = new Map();
  for (const node of previous) {
    const key = refreshKey(node);
    if (key !== null) keyed.set(key, node);
    else {
      const kind = `${node.nodeType}:${node.nodeName}`;
      if (!unkeyed.has(kind)) unkeyed.set(kind, { nodes: [], at: 0 });
      unkeyed.get(kind).nodes.push(node);
    }
  }
  const used = new Set();
  let cursor = box.firstChild;
  for (const next of [...desired.childNodes]) {
    const key = refreshKey(next);
    const group = unkeyed.get(`${next.nodeType}:${next.nodeName}`);
    let node = key === null
      ? group?.nodes[group.at++]
      : keyed.get(key);
    if (node && (used.has(node) || node.nodeName !== next.nodeName)) node = null;
    if (!node) node = next.cloneNode(true);
    used.add(node);
    if (node !== cursor) box.insertBefore(node, cursor);
    cursor = node.nextSibling;
    if (node.nodeType !== 1) {
      if (node.nodeValue !== next.nodeValue) node.nodeValue = next.nodeValue;
      continue;
    }
    if (typeof keepSelectMenu === 'function') keepSelectMenu(node, next);
    for (const attr of [...node.attributes]) {
      if (attr.name === 'open' && node.nodeName === 'DETAILS') continue;
      if (!next.hasAttribute(attr.name)) node.removeAttribute(attr.name);
    }
    for (const attr of [...next.attributes]) {
      if (attr.name === 'open' && node.nodeName === 'DETAILS') continue;
      if (node.getAttribute(attr.name) !== attr.value) node.setAttribute(attr.name, attr.value);
    }
    refreshChildren(node, next);
    if (node.nodeName === 'INPUT') {
      if (node.type === 'checkbox' || node.type === 'radio') node.checked = next.checked;
      else if (node.type !== 'file' && node.value !== next.value) node.value = next.value;
    } else if ((node.nodeName === 'SELECT' || node.nodeName === 'TEXTAREA') && node.value !== next.value) node.value = next.value;
  }
  for (const node of previous) if (!used.has(node)) node.remove();
}

function refreshPanel(box, html) {
  if (!box) return;
  const active = box.contains(document.activeElement) ? document.activeElement : null;
  const template = document.createElement('template');
  template.innerHTML = html;
  refreshChildren(box, template.content);
  if (active?.isConnected && document.activeElement !== active) active.focus({ preventScroll: true });
}
