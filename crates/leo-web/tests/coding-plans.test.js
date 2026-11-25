const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

function context() {
  const source = fs.readFileSync(path.resolve(__dirname, '../src/web/app/settings.js'), 'utf8');
  const value = { Date, rel: () => 'just now', esc: text => String(text).replace(/[<>&"]/g, ch => ({ '<': '&lt;', '>': '&gt;', '&': '&amp;', '"': '&quot;' }[ch])) };
  vm.createContext(value);
  vm.runInContext(source.slice(source.indexOf('const PLAN_STALE_MS'), source.indexOf('const THEME_LABELS')), value);
  return value;
}

test('coding accounts distinguish signed in, signed out, unknown and unavailable without changing the selection', () => {
  const value = context();
  for (const [installed, signed_in, label] of [[true, true, 'Signed in'], [true, false, 'Signed out'], [true, null, 'Sign-in unknown'], [false, null, 'Not available']]) {
    const page = { tasks: [{ task: 'writing', provider: 'ollama' }], plans: [{ provider: 'codex', name: 'Codex <img>', installed, signed_in }] };
    const html = value.codingPlans(page);
    assert.ok(html.includes(label));
    assert.equal(html.includes('Choose Codex &lt;img&gt; in Writing AI and press Sign in.'), installed && signed_in !== true);
    assert.ok(!html.includes('<img>'));
    assert.ok(!html.includes('data-action="use-plan"'));
  }
  assert.equal(value.codingPlans({ tasks: [] }), '');
});

test('usage shows both windows, clamps remaining percentages, and labels missing, stale, or expired readings', () => {
  const value = context();
  const now = Math.floor(Date.now() / 1000);
  const plan = { provider: 'codex', name: 'Codex', limits: { five_hour: { used: 0.39, resets_at: now + 3600 }, seven_day: { used: 0.06, resets_at: now + 86400 }, seen_at: new Date().toISOString() } };
  let html = value.planLimits(plan);
  for (const percent of [61, 94]) assert.ok(html.includes(`aria-valuenow="${percent}"`));
  assert.match(html, /Session/);
  assert.match(html, /Weekly/);
  assert.match(html, /Resets in/);
  assert.match(value.planUpdated(plan), /^Updated just now$/);
  assert.doesNotMatch(html, /Updated/);
  for (const [used, percent] of [[-1, 100], [0, 100], [0.95, 5], [0.39, 61], [0.06, 94], [1, 0], [2, 0]]) {
    plan.limits = { five_hour: { used, resets_at: now + 3600 }, seen_at: new Date().toISOString() };
    html = value.planLimits(plan);
    assert.ok(html.includes(`aria-valuenow="${percent}"`), `remaining ${percent}%`);
  }
  plan.limits = { five_hour: { used: 0.2, resets_at: now - 1 }, seven_day: { used: Infinity }, seen_at: 'bad-date' };
  html = value.planLimits(plan);
  assert.match(html, />Reset</);
  assert.match(html, /Not read yet/);
  assert.match(html, /Refresh to check/);
  assert.doesNotMatch(html, /aria-valuenow/);
  assert.doesNotMatch(html, /Invalid Date/);
  plan.limits = { five_hour: { used: 0.2, resets_at: now + 3600 }, seen_at: new Date(Date.now() - 600000).toISOString() };
  assert.match(value.planUpdated(plan), /^Saved · Updated/);
  for (const limits of [null, {}, ...['bad', NaN, Infinity, -Infinity, null].map(used => ({ five_hour: { used } }))]) {
    html = value.planLimits({ provider: 'codex', name: 'Codex', limits });
    assert.match(html, /Not read yet/);
    assert.match(html, /Refresh to check/);
    assert.equal(value.planUpdated({ limits }), '');
  }
  for (const resets_at of ['bad', NaN, Infinity, null, 1e300]) {
    html = value.planLimits({ ...plan, limits: { five_hour: { used: .39, resets_at } } });
    assert.match(html, /aria-valuenow="61"/);
    assert.match(html, /Reset time unavailable/);
    assert.doesNotMatch(html, /Invalid Date/);
  }
});

test('Usage does not expose writer selection controls', () => {
  const value = context();
  const page = { tasks: [{ task: 'writing', provider: 'codex' }], plans: [{ provider: 'codex', name: 'Codex', installed: true, signed_in: true }] };
  const html = value.codingPlans(page, false);
  assert.doesNotMatch(html, /data-action="use-plan"|Selected for writing/);
  assert.equal(page.tasks[0].provider, 'codex');
});
