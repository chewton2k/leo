const assert = require('node:assert/strict');
const { spawnSync } = require('node:child_process');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { test } = require('node:test');

function list(project, shard) {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), 'leo-shard-list-'));
  try {
    const args = [require.resolve('@playwright/test/cli'), 'test', '--list', '--reporter=json', `--project=${project}`];
    if (shard) args.push(`--shard=${shard}`);
    const result = spawnSync(process.execPath, args, {
      cwd: __dirname,
      env: { ...process.env, LEO_BROWSER_HOME: home },
      encoding: 'utf8',
      timeout: 30000,
      maxBuffer: 8 * 1024 * 1024,
    });
    assert.equal(result.status, 0, result.stderr || String(result.error));
    const report = JSON.parse(result.stdout);
    assert.deepEqual(report.errors, []);
    assert.equal(report.config.workers, 1, 'tests sharing a server must stay sequential');
    const specs = suite => [...(suite.specs || []), ...(suite.suites || []).flatMap(specs)];
    return report.suites.flatMap(specs);
  } finally {
    fs.rmSync(home, { recursive: true, force: true });
  }
}

function covers(project, count) {
  const all = list(project);
  const shards = Array.from({ length: count }, (_, index) => list(project, `${index + 1}/${count}`));
  const counts = shards.map(shard => shard.length);
  assert.ok(Math.max(...counts) - Math.min(...counts) <= 1, `unbalanced shards: ${counts}`);
  const ids = shards.flat().map(spec => spec.id);
  assert.equal(new Set(ids).size, ids.length, 'a test ran in more than one shard');
  assert.deepEqual(ids.sort(), all.map(spec => spec.id).sort(), 'sharding changed the selected coverage');
  return { all, shards, counts };
}

test('desktop shards balance individual tests and cover the complete suite exactly once', t => {
  const { all, shards, counts } = covers('desktop', 3);
  assert.ok(shards.filter(shard => shard.some(spec => spec.file === 'flows.spec.js')).length > 1, 'the large flow file must split across multiple shards');
  t.diagnostic(`${all.length} desktop tests across shards: ${counts.join(' / ')}`);
});

test('WebKit shards balance individual tests and cover the complete suite exactly once', t => {
  const { all, counts } = covers('webkit', 2);
  assert.ok(all.length > 0);
  t.diagnostic(`${all.length} WebKit tests across shards: ${counts.join(' / ')}`);
});

test('the phone project runs only the phone tests', () => {
  const all = list('phone');
  assert.ok(all.length > 0);
  assert.ok(all.every(spec => spec.title.includes('on a phone')), 'the phone project selected a non-phone test');
  assert.deepEqual(list('phone', '1/1').map(spec => spec.id), all.map(spec => spec.id));
});
