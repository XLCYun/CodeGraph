import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import { analyzerBuildDefinition } from '../language-packs/build-analyzer.mjs';
import { packIds, targets } from '../language-packs/distribution.mjs';

/** 构建契约逐语言选择真实 Cargo target，禁止退回共享通用 binary。 */
test('every package builds its own fixed executable on both supported targets', async () => {
  const manifest = await fs.readFile(new URL('../../Cargo.toml', import.meta.url), 'utf8');
  assert.match(manifest, /publish = false/);
  assert.match(manifest, /autobins = false/);
  for (const target of targets) {
    const entries = new Set();
    for (const id of packIds) {
      const definition = analyzerBuildDefinition(id, target);
      assert.equal(definition.binary, `codegraph-${id}`);
      assert.equal(definition.entry, `bin/codegraph-${id}${target.includes('windows') ? '.exe' : ''}`);
      assert.deepEqual(definition.args, ['scripts/cargo-agent.sh', 'build', '--locked', '--release', '--bin', `codegraph-${id}`]);
      assert.ok(manifest.includes(`name = "codegraph-${id}"`));
      entries.add(definition.entry);
    }
    assert.equal(entries.size, 7);
  }
  assert.equal((manifest.match(/\[\[bin\]\]/g) ?? []).length, 7);
  for (const id of ['codegraph', '../rust', 'typescript', 'unknown']) assert.throws(() => analyzerBuildDefinition(id, targets[0]), /identity/);
  assert.throws(() => analyzerBuildDefinition('rust', 'x86_64-unknown-linux-gnu'), /identity/);
  await assert.rejects(fs.stat(new URL('../../src/main.rs', import.meta.url)), { code: 'ENOENT' });
});
