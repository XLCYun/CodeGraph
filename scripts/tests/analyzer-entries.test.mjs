import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import { analyzerBuildDefinition } from '../language-packs/build-analyzer.mjs';
import { packIds, targets } from '../language-packs/distribution.mjs';

/** 构建选择真实语言程序；根 workspace 仅含两个薄入口，不再提供七语言 Host。 */
test('each package stages its language program and only TS Java use thin launchers', async () => {
  const root = await fs.readFile(new URL('../../Cargo.toml', import.meta.url), 'utf8');
  const launchers = await fs.readFile(new URL('../../launchers/Cargo.toml', import.meta.url), 'utf8');
  assert.match(root, /members = \["launchers"\]/);
  assert.doesNotMatch(root, /\[dependencies\]|\[package\]|\[\[bin\]\]/);
  assert.match(launchers, /publish = false/);
  for (const target of targets) {
    const entries = new Set();
    for (const id of packIds) {
      const definition = analyzerBuildDefinition(id, target);
      assert.equal(definition.binary, `codegraph-${id}`);
      assert.equal(definition.entry, `bin/codegraph-${id}${target.includes('windows') ? '.exe' : ''}`);
      entries.add(definition.entry);
      if (['rust', 'python'].includes(id)) {
        assert.equal(definition.source, `analyzers/${id}`);
        assert.deepEqual(definition.args, ['scripts/cargo-agent.sh', '+1.98.0', 'build', '--manifest-path', `analyzers/${id}/Cargo.toml`, '--locked', '--release', '--bin', `codegraph-${id}`]);
      } else if (['java', 'typescript-javascript'].includes(id)) {
        assert.equal(definition.source, 'launchers');
        assert.ok(launchers.includes(`name = "codegraph-${id}"`));
      } else assert.equal(definition.args, undefined);
    }
    assert.equal(entries.size, 7);
  }
  assert.equal((launchers.match(/\[\[bin\]\]/g) ?? []).length, 2);
  for (const id of ['codegraph', '../rust', 'typescript', 'unknown']) assert.throws(() => analyzerBuildDefinition(id, targets[0]), /identity/);
  assert.throws(() => analyzerBuildDefinition('rust', 'x86_64-unknown-linux-gnu'), /identity/);
  await assert.rejects(fs.stat(new URL('../../src', import.meta.url)), { code: 'ENOENT' });
});
