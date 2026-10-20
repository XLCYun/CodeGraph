import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { packageDirectory } from '../language-packs/build-language-support-packs.mjs';
import { packIds, targets } from '../language-packs/distribution.mjs';
import { analyzerPackageFiles, verifyAnalyzerDistribution } from '../language-packs/verify-analyzer-package.mjs';

/** 字节有效的产物仍须具有唯一固定入口、准确描述及该语言的运行时。 */
test('all fourteen package identities reject generic, foreign and incomplete payloads', async t => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'codegraph package policy-'));
  t.after(() => fs.rm(root, { recursive: true, force: true }));
  for (const target of targets) for (const id of packIds) {
    const suffix = target.includes('windows') ? '.exe' : '';
    const payload = path.join(root, `${id}-${target}`);
    const files = analyzerPackageFiles(id, target);
    const content = Object.fromEntries(files.map(name => [name, 'fixture bytes']));
    content['licenses/fixture-LICENSE'] = 'fixture license';
    content['licenses/codegraph/CodeGraph-dependencies.json'] = '[]';
    content['control/codegraph.json'] = JSON.stringify({ schema_version: 1, pack_id: id, pack_version: '0.1.0', engine_version: '0.1.0', protocol: 'codegraph.analyzer/1' });
    content['control/native.json'] = JSON.stringify({ schema_version: 1, helper_version: '0.1.0', platforms: [], ...({ go: 'go', java: 'java', csharp: 'dotnet' }[id] ? { target_sdk: { kind: { go: 'go', java: 'java', csharp: 'dotnet' }[id] } } : {}) });
    for (const [name, bytes] of Object.entries(content)) { await fs.mkdir(path.dirname(path.join(payload, name)), { recursive: true }); await fs.writeFile(path.join(payload, name), bytes, { mode: 0o755 }); }
    const output = path.join(root, 'output');
    const build = () => packageDirectory({ id, version: '0.1.0', target }, payload, output);
    await verifyAnalyzerDistribution(await build());
    for (const unwanted of [`bin/codegraph${suffix}`, `bin/codegraph-${id === 'go' ? 'rust' : 'go'}${suffix}`, `runtime/codegraph-python${suffix}`, 'bin/unrelated-tool']) {
      await fs.mkdir(path.dirname(path.join(payload, unwanted)), { recursive: true });
      await fs.writeFile(path.join(payload, unwanted), 'unwanted executable');
      await assert.rejects(verifyAnalyzerDistribution(await build()), /Unexpected package file/);
      await fs.rm(path.join(payload, unwanted));
    }
    for (const required of files) {
      await fs.rm(path.join(payload, required));
      await assert.rejects(verifyAnalyzerDistribution(await build()), /Missing package dependency/);
      await fs.writeFile(path.join(payload, required), content[required], { mode: 0o755 });
    }
    const descriptor = path.join(payload, 'control/codegraph.json');
    for (const bad of [content['control/codegraph.json'].replace('0.1.0', '9.9.9'), content['control/codegraph.json'].replace('{', '{"extra":true,'), content['control/codegraph.json'].replace('{', '{"pack_id":"wrong",')]) {
      await fs.writeFile(descriptor, bad);
      await assert.rejects(verifyAnalyzerDistribution(await build()), /descriptor/);
    }
  }
});
